# ADR 0003 — OIDC single sign-on

**Status:** accepted · **Date:** 2026-06-13 · **Scope:** F10 (OIDC / SSO)

## Context

F10 adds "log in with your company IdP" (Authentik, Keycloak, Google, …). This
is the first time Sprintly trusts an *external* party to assert who a user is,
so it introduces a real boundary: an authorization-code redirect dance, an
`id_token` whose signature we must verify against the issuer's rotating keys,
and a rule for turning issuer claims into a local `users` row. We want the
provider-specific trust handling isolated and the security-critical bits
(state/nonce/PKCE, signature, claim mapping) pure and unit-testable — before
the redirect flow accretes ad-hoc checks.

## Decision

A single configurable OIDC provider, standard **authorization code + PKCE**,
with everything security-relevant kept out of the HTTP glue.

1. **Flow state lives in a browser-bound, signed cookie — not the URL, not
   Redis.** `/auth/oidc/start` mints `state`, `nonce`, and a PKCE `verifier`,
   packs them into a short-lived (10 min) HS256 JWT signed with the existing
   `jwt_secret`, and sets it as an HttpOnly cookie (`sprintly_oidc_flow`). Only
   the `state` and the `S256(verifier)` challenge travel to the IdP. On
   `/auth/oidc/callback` we verify the cookie JWT, require the returned `state`
   to equal the cookie's, exchange the code (sending the secret `verifier`),
   and check the `id_token`'s `nonce` against the cookie's. The verifier never
   appears in a URL or reaches the IdP, so it can't leak via logs or referers;
   binding `state` to an HttpOnly cookie defeats login-CSRF without server-side
   session storage.

2. **`id_token` validation is a pure function over a fetched JWKS.** We fetch
   the issuer's discovery document and JWKS, pick the key by `kid`, build a
   `jsonwebtoken` `DecodingKey` from the RSA `n`/`e`, and validate RS256 +
   `iss` + `aud` + `exp` + `nonce`. The validator takes the JWKS and clock as
   arguments, so tests mint a token with a throwaway key and assert that
   tampered signatures, wrong issuer/audience/nonce, and expiry are rejected —
   no network, matching the codebase's no-HTTP-mock test style.

3. **Outbound HTTP reuses the curl-subprocess pattern (ADR 0001), not a new
   client.** Discovery and JWKS are `curl` GETs; the token exchange is a
   `curl` POST whose body — which carries `client_secret` and the PKCE
   `verifier` — is piped via **stdin**, never argv, so it can't be read from
   the process table. Adding `reqwest` for three requests would contradict
   ADR 0001 and inflate the runtime image; the curl machinery is already
   proven for webhooks and provider status.

4. **Claims → user is create-or-link, keyed by federated identity.** New
   columns `users.oidc_issuer` + `users.oidc_subject` (partial-unique together)
   store the federation key. On callback we resolve, in order: existing user by
   (`issuer`, `subject`) → existing **active** user by verified email (link it,
   stamping the subject) → otherwise create a fresh `member`. An optional
   email-domain allowlist gates creation *and* linking. Federated-only users
   get a random unusable password hash (the column is `NOT NULL`); local login
   simply never matches it.

5. **Local login coexists by default.** Password login keeps working unless
   `SPRINTLY_LOCAL_LOGIN_DISABLED=true`, which makes `/auth/login` refuse with
   a clear message and leaves SSO as the only door. OIDC is *enabled* only when
   issuer + client id + client secret are all configured; absent that, the
   endpoints 404-equivalent (return "not configured") and the UI hides the SSO
   button.

## Alternatives considered

- **Server-side (Redis) flow state.** Works, but adds a round-trip and a
  storage lifecycle for data that fits in a signed, self-expiring cookie. The
  cookie is browser-bound (better CSRF story) and keeps `/callback` stateless.
- **A full OIDC client crate (`openidconnect`).** Pulls a large dependency
  tree (and `reqwest`) for one provider and one flow. The pieces we need —
  discovery JSON, PKCE S256, RS256-against-JWKS — are small and already
  expressible with `jsonwebtoken` + curl; rolling them keeps the dep budget
  flat and the validation auditable.
- **Encrypt the verifier into the URL `state`.** Avoids a cookie, but any URL
  capture (proxy log, referer) plus our key would expose it; an HttpOnly
  cookie is strictly safer and no more code.
- **Mock issuer via `wiremock` in CI.** The security logic is the signature
  and claim checks, which we test directly against a self-minted token; the
  curl round-trip is thin glue verified manually. Avoids a dev-dependency and
  matches how the rest of the suite tests outbound HTTP (it doesn't mock it).

## Consequences

- Supporting a second simultaneous provider would mean widening config to a
  list and threading a provider id through the flow cookie and callback — out
  of scope for F10, but the pure validator/mapping don't change.
- We trust the IdP's `email_verified`; unverified emails are not auto-linked to
  existing accounts (they create a new federated user instead, or are rejected
  by the allowlist), so a hostile IdP can't take over a local account by
  asserting its email.
- Key rotation is handled by fetching JWKS per validation attempt (cheap, and
  correct across rotations); a future optimisation can cache by `kid` with a
  short TTL without changing the validator's signature.
- The redirect URLs are derived from `SPRINTLY_PUBLIC_URL`
  (`/api/v1/auth/oidc/callback`) so a single-origin deploy needs no extra
  config beyond issuer + client credentials.
