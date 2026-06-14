# Security

> Stub. The full version lands with the vault (M7). This file documents what
> we have *now* (M1 phase 1) and what we promise for later.

## Now

- All secrets live in env vars. `.env` is gitignored. `.env.example` is the
  only env file checked in and only contains placeholders.
- Postgres / Redis / MinIO are not exposed to the public internet — Caddy is
  the only public surface. In dev they bind to `127.0.0.1` only.
- Caddy ships strict security headers (HSTS, X-Content-Type-Options, frame
  deny, referrer policy).
- The API refuses to boot if `SPRINTLY_JWT_SECRET` decodes to fewer than 32
  bytes or `SPRINTLY_VAULT_MASTER_KEY` isn't exactly 32 bytes.
- All SQL is parameterised via SQLx. No string interpolation, ever.

## Shipped in M1 phase 2 (auth flow)

- **Argon2id** password hashing, parameters from env, tunable per target
  hardware. Login does a constant-time dummy verify on unknown-email so
  timing can't enumerate accounts.
- **Access tokens:** JWT HS256, ~15 min TTL, carry `sub`/`sid`/`role`/`iat`/`exp`.
- **Refresh tokens:** opaque 32-byte URL-safe-base64 string. We store
  SHA-256(secret) in `refresh_tokens.token_hash`; the plaintext only lives
  in the user's cookie.
- **Rotation + reuse detection:** every refresh use mints a new token and
  marks the old row `rotated_to`. If we ever see a presented token whose
  `rotated_to` is non-NULL, the *entire* session family is revoked — every
  refresh token in it stops working, on the spot. Password reset does the
  same across all of a user's sessions.
- **Session-liveness check** on every authenticated request — revoked
  sessions stop working immediately, not at JWT expiry. One cheap SELECT.
- **Cookies:**
  - `sprintly_access`  — JWT, `HttpOnly; SameSite=Lax`, scoped `Path=/`.
  - `sprintly_refresh` — opaque, `HttpOnly; SameSite=Lax`, scoped
    `Path=/api/v1/auth` so it never rides on non-auth requests.
  - `Secure` flag set automatically when `SPRINTLY_PUBLIC_URL` begins with
    `https://`.
- **Registration gate:** first user becomes admin. After that, registration
  requires either `SPRINTLY_OPEN_SIGNUP=true` or a valid invite token.
  Invite tokens are single-use; their plaintext is never stored.
- **Password reset:** time-bounded tokens (30 min), single-use, plaintext
  surfaced in the dev response only. Consuming a token revokes every active
  session for that user.
- **RBAC:** single `can(actor, action, resource)` function in
  `domain::permissions`. Adding an `Action` variant without matching it is
  a compile error.
- **Public status pages (F18) are opt-in and whitelisted.** A project lead can
  mint a `projects.public_token`; `GET /public/status/<token>` is the only
  unauthenticated read endpoint and returns a hand-built struct
  (`domain::public_status::PublicStatus`) carrying *only* the project name, the
  active sprint's progress, and per-column task **counts** — never task content,
  assignees, labels, custom fields, comments, attachments, or vault data.
  Disabling clears the token and the URL 404s.

## Invite-token lifecycle

| Step      | What happens                                                                                    |
| --------- | ----------------------------------------------------------------------------------------------- |
| Create    | Admin calls `POST /admin/invites` → server mints 32 random bytes, stores `sha256` only, returns plaintext + URL **once**. |
| Distribute| Admin copies the URL to the recipient out-of-band (Slack DM, whatever).                         |
| Consume   | Recipient hits `POST /auth/register` with `invite_token` — server hashes presented token, matches, stamps `consumed_at`. |
| Revoke    | Admin calls `POST /admin/invites/:id/revoke` → `expires_at` collapses to now; row preserved for audit. |

Plaintext is never logged, never written to disk, and never returned a second
time. If the recipient loses the link, revoke and re-issue.

## Shipped in M3-B (attachments)

- **Two-phase upload via presigned PUT.** The API never streams file bytes —
  it issues a 10-minute presigned `PUT` URL bound to a specific bucket key
  (`tasks/{task_id}/{attachment_id}`). The browser uploads directly to MinIO.
  On success the client POSTs to `/attachments/:id/complete`, which flips
  status `pending → ready` and records the size + checksum.
- **Presigned signer is hand-rolled SigV4** in `apps/api/src/infra/s3.rs`. No
  aws-sdk-s3 dep. `SignedHeaders=host`, `UNSIGNED-PAYLOAD`, 600s default
  expiry. Unit-tested on URL shape.
- **Downloads** use presigned GETs with `response-content-disposition` set to
  `attachment; filename=...` so the browser saves under the original name.
- **Orphaned `pending` rows** are pruned by a background job (table is in
  place; runner lands when there's a second job to share infra).
- **Delete is soft.** The MinIO object is collected by a background sweep,
  not synchronously — the API never blocks on storage I/O.

## Shipped in M3-B (markdown)

- All user-supplied markdown is rendered through `rehype-sanitize` with the
  default schema. No `dangerouslySetInnerHTML` anywhere in the codebase.
- Code blocks render with monospace + no execution context. Link clicks
  open in a new tab with `rel="noreferrer noopener"`.

## Shipped in M2 (CSRF)

Double-submit cookie pattern.

- On login / refresh, the API sets `sprintly_csrf` — a 24-byte random nonce,
  `SameSite=Lax`, **not** `HttpOnly` (the browser JS needs to read it).
- The frontend fetch wrapper (`apps/web/lib/api.ts`) mirrors that cookie into
  the `X-CSRF-Token` header on every `POST`/`PATCH`/`PUT`/`DELETE`.
- Middleware (`apps/api/src/middleware/csrf.rs`) compares header vs cookie
  in constant time. Mismatch → `403 csrf`.
- Exempt by path: `/auth/login`, `/auth/register`, `/auth/refresh`,
  `/auth/password/reset/{request,confirm}` — these run before the cookie is
  set, and the refresh cookie's `SameSite=Lax + Path=/api/v1/auth` already
  blocks cross-origin requests for modern browsers.
- Exempt by transport: any request with `Authorization: Bearer ...` skips the
  check — those don't ride on cookies, so they aren't CSRF-vulnerable.

## Shipped in M10 (admin + ops)

- **`admin_audit_log`** is append-only — same `sprintly_block_audit_mutation`
  trigger that protects the vault. Every admin write goes through
  `routes::admin_panel::write_admin_audit` which also resolves the real
  client IP via the X-Forwarded-For middleware.
- **X-Forwarded-For trust.** Sprintly deploys behind exactly one reverse
  proxy (Caddy), which overwrites the header on ingress. Hostile clients
  can put anything in XFF — Caddy will replace it. The resolver walks the
  list and picks the first non-private hop; fallback to the socket address.
- **Suspending a user** sets `status='suspended'` AND revokes every active
  session (`sessions.revoked_at = now()`). The session-liveness check on
  every request means the suspended user is signed out within one
  request-round-trip.
- **Backups via pg_dump → MinIO.** The `create_backup` job is enqueued from
  the admin panel and run by the in-process worker (`apps/api/src/jobs/mod.rs`).
  Output object: `s3://sprintly/backups/YYYY-MM-DD/<backup_id>.dump`. Custom
  format with `-Z 6` compression. Failures land on the `backups` row's
  `error` field with the `pg_dump` stderr.
- **Scheduled backups + retention (F15).** Set `SPRINTLY_BACKUP_SCHEDULE_SECS`
  and the worker auto-creates a backup on that cadence (the same code path as
  the manual button). `SPRINTLY_BACKUP_RETENTION_COUNT` / `_DAYS` enable a
  retention sweep (every 6h) that prunes the MinIO object **and** the row for
  backups beyond the policy. Both are opt-in — unset means "keep everything",
  the old behaviour. The newest backup is always kept as a safety floor.
- **Restore is a guarded operator action**, not a UI button — this avoids
  one-click data loss. The streamlined path is the CLI below; the fully-manual
  path remains for unusual cases.
- **Webhooks scaffold.** Rows + per-project CRUD + secret-hash-on-disk are
  in place. Outbound delivery is intentionally not wired in v1 — the admin
  UI flags this as "Coming soon".

### Backup restore runbook

**Streamlined (CLI, F15).** The `restore` subcommand downloads the chosen
backup from MinIO and runs `pg_restore --clean --if-exists` against the target
database. It demands an explicit `--confirm` (it overwrites data) and writes a
`backup.restore` row to the admin audit log.

1. Find the backup id in the admin panel (Backups tab) or `GET /admin/backups`.
2. **Drill on staging first.** Point at a throwaway DB by exporting
   `SPRINTLY_RESTORE_DATABASE_URL=postgres://…/sprintly_staging` (defaults to
   `DATABASE_URL` if unset).
3. Dry-run the guard (no `--confirm`) to see what it would do:
   `docker compose -f infra/compose/docker-compose.yml exec api sprintly-api restore <backup_id>`
4. Execute: append `--confirm`:
   `… exec api sprintly-api restore <backup_id> --confirm`
5. For an in-place production restore, stop the app first
   (`docker compose stop web caddy`), run the command against `DATABASE_URL`,
   then `docker compose up -d`.

**Fully manual (fallback).** If you'd rather drive it by hand:

1. From an admin shell: `docker compose -f infra/compose/docker-compose.yml exec api bash`
2. Identify the backup in MinIO: console at `:9001` → bucket `sprintly` →
   `backups/YYYY-MM-DD/<backup_id>.dump`. Download to your laptop.
3. Stop the API but keep Postgres running:
   `docker compose stop api web caddy migrate`
4. Drop and recreate the database:
   `docker compose exec postgres psql -U sprintly -c "DROP DATABASE sprintly; CREATE DATABASE sprintly;"`
5. Restore (from inside the api container so the network reaches Postgres):
   `pg_restore --no-owner --no-acl --dbname=$DATABASE_URL /tmp/<backup_id>.dump`
6. Restart everything: `docker compose up -d`. Migrations run as the
   one-shot `migrate` service and exit; the API picks up from the restored
   schema.

## Still to come

- **TOTP 2FA (RFC 6238)** with backup codes. Required for admin, optional
  for members, project-configurable for vault access. Schema columns are
  in place (`totp_secret`, `totp_enrolled_at`, `backup_codes`); enrollment
  endpoint lands when the admin panel does (M10).
- **CSRF nonce** on browser-initiated writes — *shipped in M2*. See below.

## Coming in M7 (vault)

- Master key in env, never on disk in plaintext.
- HKDF-SHA256 per-project key derivation.
- XChaCha20-Poly1305 per-item encryption, fresh 24-byte nonce per write.
- Audit log row per reveal/copy/edit, append-only.
- Reveal endpoint rate-limited; revealed values never enter Zustand or
  localStorage on the client; clipboard cleared after 30 seconds.

## Threat model (sketch)

The full threat model lives here once the vault ships. Three actors to think
about:

1. **A reader of a DB dump.** Sees ciphertext for vault items, sees
   `argon2id` password hashes, sees session/refresh hashes. Cannot recover
   plaintexts without the master key.
2. **An attacker with API code execution.** Game over for vault contents —
   document this clearly. We mitigate via process isolation, no shell, no
   debug endpoints in prod.
3. **A malicious project lead.** Sees what their role allows. Audit log
   makes their actions visible. Cannot escape their project boundary.

## Targets

- OWASP ASVS Level 2.
- All write endpoints require a CSRF nonce when called from a browser
  session.
- File uploads MIME-sniffed server-side.
- `/metrics` (Prometheus) only behind admin-only flag.
