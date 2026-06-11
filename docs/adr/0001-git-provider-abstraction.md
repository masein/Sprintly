# ADR 0001 — Git provider abstraction boundary

**Status:** accepted · **Date:** 2026-06-11 · **Scope:** F1 (GitHub / GitLab / Gitea), F3 (CI status)

## Context

F1 slices 1–2 (#27, #28) shipped GitHub-only: one webhook route, signature
verification against a single global env secret, and GitHub-payload parsing
inlined in the route handler. Remaining F1 work needs per-project
connections, outbound commit-status calls, and two more providers. We need
to decide what varies per provider and what is shared, before the
GitHub-shaped code calcifies.

## Decision

A provider is a **stateless adapter** behind three narrow seams, dispatched
by a plain `enum Provider { Github, Gitlab, Gitea }` (`match`, no trait
objects). Everything else — task-key parsing, linking, transitions, job
running, storage — is shared core and provider-blind.

1. **Inbound (webhook → events):** per provider, two pure functions:
   - `verify(secret, headers, body) -> bool` — GitHub/Gitea: HMAC-SHA256 of
     the body (`X-Hub-Signature-256` / `X-Gitea-Signature`); GitLab:
     constant-time equality of `X-Gitlab-Token`.
   - `parse(headers, body) -> Vec<GitEvent>` — maps the provider payload to
     the neutral event model. `GitEvent` is the contract:
     `Push { branch, commits: [{sha, message, url}] }` and
     `PullRequest { number, title, body, url, state, head_sha }`.
   The shared handler then runs: extract task keys → link → transition.

2. **Outbound (status → HTTP request):** per provider, one pure builder
   `status_request(base_url, repo, token, sha, status) -> HttpRequest`
   (method + url + headers + body as data). A shared job executes it with
   the same curl-subprocess machinery the webhook deliverer uses. GitHub and
   Gitea share the `POST /repos/{repo}/statuses/{sha}` shape; GitLab is
   `POST /api/v4/projects/{repo}/statuses/{sha}` with a token header.

3. **Connection/auth (storage):** one `git_integrations` row per
   (project, provider, repo): optional `base_url` for self-hosted
   instances, webhook secret and API token vault-encrypted (XChaCha20 via
   the existing per-project key derivation, AAD = integration id). Inbound
   routes resolve the integration by id in the URL; no global secrets.

## Alternatives considered

- **`trait GitProvider` with async methods / dyn dispatch.** Three known
  providers, all pure logic; an enum match is shorter, monomorphic, and
  keeps the pure functions trivially unit-testable. A trait buys
  extensibility we don't need and complicates async signatures.
- **Separate full handler per provider (status quo extended).** Fastest to
  write, but linking/transition logic would fork three ways and drift —
  slice 2 already showed the handler accreting business logic.
- **Outbound via a Rust HTTP client (reqwest).** New dependency for one
  POST shape; the curl-subprocess pattern is already proven in the webhook
  deliverer and keeps the dependency budget flat.

## Consequences

- Adding a provider = one module with two pure inbound functions + one
  outbound builder + a `Provider` variant. Integration tests feed recorded
  fixtures through the shared handler.
- `GitEvent` is deliberately minimal; richer events (reviews, checks) extend
  the enum rather than leaking provider payloads into core. F3 adds a
  `CheckStatus`-shaped event the same way.
- The legacy global-secret GitHub route stays until per-project connections
  are the norm, then is removed with a deprecation note.
- Provider API differences (e.g. GitLab project-id encoding) live entirely
  in the adapter; the core never sees a provider payload.
