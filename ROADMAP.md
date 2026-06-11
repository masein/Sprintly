# Sprintly Roadmap

Self-hosted, developer-themed project management for small software teams.
This file is the **single source of truth** for planned work. Each item is a
checkbox with a complete, self-contained spec — enough to implement it well
from this document alone.

## How we work

- **One item → one branch → one PR.** Branch names: `feat/<id>-slug` or
  `chore/<id>-slug` (e.g. `feat/f1-git-integration`, `chore/h1-rate-limit`).
- The PR that implements an item **ticks that item's checkbox** in this file
  and fills in its **PR:** link.
- `main` is protected: PRs require the `api · fmt · clippy · test` and
  `web · lint · build` checks to pass and the branch to be up to date.
- Work top-to-bottom within a milestone; respect the **Depends on** field.

## Global Definition of Done (applies to every item)

An item is `[x]` only when **all** of these hold:

1. Acceptance criteria met.
2. Tests written per the item's **Test plan** and green locally + in CI.
3. `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo test --workspace` pass (backend); `pnpm lint`/`typecheck`/`build`
   pass (web).
4. If queries changed: `apps/api/.sqlx` regenerated and committed.
5. If schema changed: reversible migration (`*.up.sql` + `*.down.sql`) added.
6. User-facing behaviour documented (in-app `/docs` and/or `README.md`).
7. Personality respected (see `docs/PERSONALITY.md`): dry voice, no manager
   surveillance of personal signals, color never the only cue.
8. This file updated: box checked, **PR:** link filled.

## Testing conventions

- **Rust unit tests:** in-module `#[cfg(test)]`, pure logic (no DB).
- **Rust integration tests:** `apps/api/tests/*.rs` using
  `#[sqlx::test(migrations = "./migrations")]` (isolated per-test database).
  Reuse the existing `make_user` / `make_project` helpers; derive unique
  emails/handles from `id.simple()`.
- **Web e2e:** Playwright specs in `apps/e2e/tests/*.spec.ts`, driven through
  Caddy at `:8080`. Each new top-level surface gets at least one smoke spec.
- **Acceptance tests** below are written so they map 1:1 to an integration or
  e2e test case.

## Status legend

`[ ]` not started · `[~]` in progress (PR open) · `[x]` done (merged).

---

# M11 — Harden

> Stabilize security, reproducibility, and CI before building new surface area.

### `[x]` H1 — Auth rate limiting
**PR:** #4 · **Depends on:** none · **Size:** S

**Goal:** Throttle credential-guessing on `POST /auth/login` and reset abuse on
`POST /auth/password/reset/request`.

**Why:** Vault reveal is already rate-limited (`routes/vault.rs`,
`REVEAL_LIMIT_PER_HOUR`), but auth endpoints have none — `middleware/mod.rs`
explicitly defers it. Brute force and reset-spam are open today.

**Design:** Reuse the vault reveal token-bucket pattern (Redis). Add a small
`middleware::rate_limit` helper: `check(redis, key, limit, window) -> Result<()>`
returning `AppError::TooManyRequests` (HTTP 429) when exceeded.
- Login key: `rl:login:{ip}` and `rl:login:{email_lower}` — both checked; e.g.
  10/min/IP and 5/min/email. On success, optionally reset the email counter.
- Reset-request key: `rl:reset:{ip}` and `rl:reset:{email_lower}`, e.g. 5/hour.
- Use the X-Forwarded-For-aware client IP resolver (`middleware::client_ip`).

**Surface changes:** add `AppError::TooManyRequests` → 429 with
`{ "error": { "code": "rate_limited", "message": "Slow down. Try again in a bit." } }`
and a `Retry-After` header.

**Acceptance criteria:**
1. ≤ N login attempts/min from one IP succeed-or-fail normally; the (N+1)th
   returns 429 with `Retry-After`.
2. Per-email throttle triggers independent of IP.
3. Reset-request endpoint throttles per IP and per email; response body stays
   the neutral "if that account exists…" message (no enumeration).
4. Limits are configurable via env with sane defaults; documented.
5. 429 copy matches the personality guide.

**Test plan:**
- Unit: token-bucket helper (allows up to limit, denies after, refills after window — use injected clock/window).
- Integration (`tests/auth_sessions.rs`): loop login attempts past the limit, assert 429 + `Retry-After`; assert a different IP/email is unaffected.
- e2e: optional — login form shows the rate-limit message.

---

### `[x]` H2 — Fail-loud config + secret hygiene
**PR:** #5 · **Depends on:** none · **Size:** S

**Goal:** Make `Config::from_env()` failures visible and inputs forgiving.

**Why:** Config is parsed before the tracing subscriber initialises, so a bad
env var exits 1 with **no log** (a stray `%` in a base64 secret cost real debug
time). Base64 decode is also strict about surrounding whitespace.

**Design:**
- In `main.rs`, if `Config::from_env()` returns `Err`, `eprintln!` a clear
  message to stderr **before** any logging init, then exit non-zero.
- In `config.rs`, trim ASCII whitespace/control chars from base64 inputs before
  decoding; on failure, name the offending var and the expected shape.
- Add a `sprintly-api check-config` subcommand that validates env and prints a
  redacted summary (lengths, not values), exit 0/1.

**Acceptance criteria:**
1. Booting with a malformed `SPRINTLY_JWT_SECRET` prints a specific stderr line
   naming the var; exit code ≠ 0.
2. A secret with trailing whitespace/newline still boots.
3. `sprintly-api check-config` exits 0 on a valid env and prints redacted info;
   exits 1 and explains on an invalid one.

**Test plan:**
- Unit (`config.rs`): `from_env`-style helper over a map fixture — valid,
  whitespace-padded (ok), malformed (named error), too-short key (named error).
- Manual/integration: run the binary with a bad env in CI-like shell; assert
  stderr contains the var name.

---

### `[x]` H3 — Commit `pnpm-lock.yaml`
**PR:** #6 · **Depends on:** none · **Size:** S

**Goal:** Reproducible web builds and cacheable CI installs.

**Why:** No lockfile exists; web builds float, and CI couldn't use `cache: pnpm`.

**Design:** Generate `pnpm-lock.yaml` at the workspace root, commit it, flip
Docker/web install to `--frozen-lockfile=true`, restore `cache: pnpm` (with
`cache-dependency-path: pnpm-lock.yaml`) in `ci.yml`, and un-ignore the lockfile
in `.gitignore` if needed.

**Acceptance criteria:**
1. `pnpm-lock.yaml` committed; `pnpm install --frozen-lockfile` succeeds.
2. Web Docker build and CI use the frozen lockfile; CI restores the pnpm cache.
3. Clean `pnpm build` still passes.

**Test plan:** CI is the test — both jobs green with cache restored.

---

### `[x]` H4 — `just test` / `just lint` work against the running stack
**PR:** #7 · **Depends on:** none · **Size:** S

**Goal:** The documented test/lint recipes actually run.

**Why:** They `docker compose exec api cargo …`, but the prod `runtime` image
has no Rust toolchain — only the `dev` target does, and `just up` excludes the
override. So `just test` fails against a `just up` stack.

**Design:** Add a `test` compose profile (or a dedicated one-shot service built
from the `dev` target with the source mounted and a Postgres test DB) and point
`just test`/`just lint`/`just sqlx-prepare` at it. Document the flow.

**Acceptance criteria:**
1. From a fresh `just up`, `just test` runs the full backend suite to green.
2. `just lint` runs fmt + clippy + web lint without manual setup.
3. README "Quickstart" updated.

**Test plan:** Manual: documented commands run clean on a fresh checkout.

---

### `[x]` H5 — CI hardening
**PR:** #8 · **Depends on:** H3 · **Size:** M

**Goal:** Catch regressions the current CI can't.

**Design / scope:**
- `cargo sqlx prepare --check` step — fail if `.sqlx` is stale vs queries.
- `cargo audit` (RUSTSEC) and `pnpm audit` (non-blocking → warn first, then
  enforce) for dependency CVEs.
- Dependabot config for cargo + npm + GitHub Actions.
- Wire the Playwright e2e job: boot the stack (compose) and run `apps/e2e`.
- Bump pinned actions off Node-20 deprecation.

**Acceptance criteria:**
1. A deliberately stale `.sqlx` fails CI.
2. Audit jobs run and surface advisories.
3. Dependabot opens dependency PRs.
4. e2e job runs the smoke spec against a live stack and passes.

**Test plan:** CI is the test; verify each new job on the PR.

---

### `[x]` H6 — Correct the Rust MSRV
**PR:** #12 · **Depends on:** none · **Size:** S

**Goal:** Declared `rust-version` matches reality.

**Why:** `Cargo.toml` says `1.85`, but locked deps require `1.88` (Docker/CI use
1.88). The comment is misleading.

**Design:** Set `rust-version = "1.88"` and update the explanatory comment.

**Acceptance criteria:** Build/clippy/test still green on 1.88; comment accurate.

**Test plan:** CI green.

---

### `[x]` H7 — Slim the `migrate` subcommand
**PR:** #23 · **Depends on:** none · **Size:** S

**Goal:** `migrate` should need only `DATABASE_URL`.

**Why:** `cmd_migrate` builds the full `Config`, so the compose `migrate` service
needs the entire app env (worked around by duplicating env). A migration step
shouldn't need the JWT/vault secrets.

**Design:** Make `cmd_migrate` read `DATABASE_URL` directly (and `RUST_LOG`)
instead of `Config::from_env()`; trim the compose `migrate` env back to the DB
URL.

**Acceptance criteria:**
1. `migrate` runs with only `DATABASE_URL` + `RUST_LOG` set.
2. `just up` migrate one-shot still completes successfully.

**Test plan:** Integration/manual: run the migrate container with a minimal env;
assert exit 0 and schema applied.

---

# M12 — Comms backbone

> Reach users. Everything later (mentions, PR events, digests) rides this.

### `[x]` F4 — Transactional email
**PR:** #24 · **Depends on:** none · **Size:** M

**Goal:** Send real email for password reset, invites, and (later) notifications.

**Design:**
- `infra::email` trait with an SMTP implementation (`lettre`) and a `log`
  implementation for dev (prints to stdout — preserves today's behaviour).
- Configurable via env (`SPRINTLY_SMTP_URL`, `SPRINTLY_MAIL_FROM`); when unset,
  fall back to the `log` sender.
- Templated plain-text + minimal HTML; templates live in `apps/api/templates`.
- Wire into: reset-request, invite creation. Sends are best-effort + logged;
  failures don't 500 the request.

**Acceptance criteria:**
1. With SMTP configured, a reset request delivers an email containing a working
   token link; without SMTP, the dev sender logs it (no 500 either way).
2. Invite emails send to the invitee.
3. Email never blocks the HTTP response path > a small timeout.

**Test plan:**
- Unit: template rendering (token interpolation, escaping).
- Integration: a mock/`log` sender records the payload; reset-request enqueues
  one mail to the right address with a valid token.

---

### `[x]` F5 — In-app notifications + `@mentions` + watcher fan-out
**PR:** #25 · **Depends on:** F4 · **Size:** L

**Goal:** Users get notified of mentions, assignments, and watched-task changes.

**Design:**
- `notifications` table (id, user_id, kind, payload jsonb, read_at, created_at).
- Producers: comment `@handle` parse, task assignment, watched-task activity,
  retro promote. Fan-out via the existing background worker / WS channel.
- `GET /me/notifications`, `POST /me/notifications/:id/read`,
  `POST /me/notifications/read-all`, unread count on WS.
- Frontend: header bell + dropdown center; mark-read; deep links.
- Optional email digest hook (uses F4); respect per-user prefs in settings.

**Acceptance criteria:**
1. `@handle` in a comment creates a notification for that user (not the author).
2. Assigning a task notifies the assignee; watchers get activity notifications.
3. Unread count updates live over WS; marking read clears it.
4. A user never notifies themselves for their own action.

**Test plan:**
- Unit: mention parser (handles, dedupe, unknown handles ignored, code spans skipped).
- Integration: comment with mention → row created for mentioned user only;
  assignment → assignee notified; read endpoints flip `read_at`.
- e2e: bell shows unread badge after a mention; clicking marks read.

---

### `[x]` F2 — Outbound webhook delivery + chat notifications
**PR:** #26 (generic signed+retried delivery; Slack/Discord adapters deferred) · **Depends on:** F5 · **Size:** M

**Goal:** Actually deliver the webhooks that are currently only stored, plus
first-class Slack/Discord targets.

**Design:**
- Delivery worker: on domain events (task moved/created, sprint completed, etc.)
  enqueue `webhook_deliveries`; POST JSON with an HMAC-SHA256 signature header
  using the stored hashed secret; exponential backoff + max attempts; record
  status/response.
- Slack/Discord adapters format the event into their message JSON.
- `GET /projects/:key/webhooks/:id/deliveries` for debugging; "send test" action.

**Acceptance criteria:**
1. A subscribed event triggers a signed POST to the endpoint; signature verifies
   against the secret.
2. Failed deliveries retry with backoff and are visible with status.
3. Slack/Discord URLs receive a correctly-formatted message.
4. "Send test event" works from the admin UI.

**Test plan:**
- Unit: signature computation; event→payload mapping; Slack/Discord formatting.
- Integration: event enqueues delivery; a stub receiver asserts headers + body;
  forced failure schedules a retry.

---

# M13 — Dev integration ⭐

> The flagship differentiator: tie the board to the codebase.

### `[~]` F1 — Git provider integration (GitHub / GitLab / Gitea)
**PRs:** #27 (inbound webhook commit/PR → task linking), #28 (auto-transition on merge + linked-git panel) · remaining: OAuth app + outbound commit status (unblocks F3) + multi-provider · **Depends on:** F2, F5, F12 · **Size:** L

**Goal:** Link branches/PRs/commits to tasks, auto-transition on merge, surface
PR state on cards. Make `PR_WIZARD` mean something real.

**Design:**
- Per-project provider connection (`git_integrations`: provider, repo, secrets,
  webhook secret). Inbound webhook endpoint per provider verifying signatures.
- Parse smart references in branch names / commit messages / PR titles
  (`DEMO-1`); link to tasks (`task_git_links`).
- Events: PR opened/merged/closed → comment on task + optional column transition
  (configurable mapping, e.g. merged → Done); commit pushed → activity entry.
- Card badge shows PR status (open/merged/checks). Achievement `PR_WIZARD`
  recomputed from real merged PRs.
- Outbound (optional later): create branch / open PR from a task.

**Acceptance criteria:**
1. Connecting a repo and pushing a branch named `DEMO-1-foo` links it to DEMO-1.
2. Merging a linked PR moves the task per the configured mapping and comments.
3. Inbound webhook signatures are verified; bad signatures rejected.
4. `PR_WIZARD` reflects merged-PR count, not done-task count.

**Test plan:**
- Unit: reference parser (branch/commit/PR variants, multiple keys, false positives).
- Integration: simulated provider webhook payloads (GitHub + GitLab fixtures) →
  link created, transition applied, signature enforced.
- e2e: card shows a PR badge after a simulated merge.

> **Design spike first:** write a short ADR choosing the abstraction boundary
> between providers before coding (kept in `docs/adr/`).

---

### `[ ]` F3 — CI/CD status on tasks & PRs
**PR:** _none yet_ · **Depends on:** F1 · **Size:** M

**Goal:** Show build/check status from the provider on linked PRs/cards.

**Design:** Consume check/status webhook events from F1's provider connection;
store latest status per linked PR; render a pass/fail/pending chip (with icon +
label, never color-only).

**Acceptance criteria:**
1. A check event updates the linked PR's status chip.
2. Pending/passed/failed render with distinct icon + label.

**Test plan:** Integration: check-status webhook fixture updates stored status;
unit: status→chip mapping.

---

# M14 — Planning & views

> Scale beyond a single board.

### `[x]` F7 — Labels / tags + custom fields
**PR:** #40 (labels registry + colors) + #44 (custom fields) · **Depends on:** none · **Size:** M

**Goal:** Flexible categorisation and per-project custom fields on tasks.

**Design:** `labels` (per project, name+color) + `task_labels`; `custom_fields`
(project-scoped: text/number/select/date) + `task_field_values`. Filter chips +
search integration. CRUD endpoints + board filter support.

**Acceptance criteria:**
1. Create/assign labels; filter board + search by label.
2. Define a custom field; set it on a task; it persists and is filterable.
3. Color always paired with the label text.

**Test plan:** Integration: label CRUD + filter query; custom field set/read +
type validation. Unit: filter DSL parsing for labels/fields.

---

### `[ ]` F8 — Saved filters / board views / swimlanes
**PR:** _none yet_ · **Depends on:** F7 · **Size:** M

**Goal:** Named, shareable board views with grouping (swimlanes) and filters.

**Design:** `board_views` (owner/shared, filter json, group-by). Board renders
swimlanes by assignee/label/priority; quick-switch in the board header.

**Acceptance criteria:**
1. Save a filtered view; reopen restores filters + grouping.
2. Shared views are visible to project members.
3. Swimlane grouping by assignee/label/priority works.

**Test plan:** Integration: view CRUD + access scoping. e2e: switch views on the
board.

---

### `[ ]` F6 — Roadmap / timeline
**PR:** _none yet_ · **Depends on:** F7 · **Size:** L

**Goal:** Epics + milestones with a timeline (Gantt-lite) view.

**Design:** `epics` (project-scoped, name, dates, color); tasks reference an
epic; `milestones` (date + target). Timeline view renders epics/milestones over
a date axis; drag to reschedule (optional v2).

**Acceptance criteria:**
1. Create epics + milestones; assign tasks to an epic.
2. Timeline renders epics as bars and milestones as markers by date.
3. Epic progress = done/total of its tasks.

**Test plan:** Integration: epic/milestone CRUD + task→epic association +
progress rollup. e2e: timeline renders an epic bar.

---

### `[ ]` F9 — Recurring tasks & templates + backlog/bulk ops
**PR:** _none yet_ · **Depends on:** none · **Size:** M

**Goal:** Reduce repetitive setup; manage the backlog efficiently.

**Design:** `task_templates` (project-scoped task skeletons); recurrence rule on
a template (`recurrence`: none/daily/weekly/monthly) materialised by the worker.
Backlog view (unscheduled/no-sprint tasks) with multi-select bulk actions
(assign, label, move to sprint/column, delete).

**Acceptance criteria:**
1. Create a task from a template; fields prefill.
2. A weekly-recurring template materialises a task each week via the worker.
3. Bulk-select N backlog tasks and assign them to a sprint in one action.

**Test plan:** Integration: template→task instantiation; recurrence materialisation
(advance clock); bulk op applies to all selected. Unit: recurrence next-date calc.

---

# M15 — Identity

> Enterprise-ready self-hosting.

### `[ ]` F12 — Personal API tokens
**PR:** _none yet_ · **Depends on:** none · **Size:** M

**Goal:** Scriptable, scoped access to the REST API (also enables F1/F2 automation).

**Design:** `api_tokens` (user_id, name, hashed token, scopes, last_used_at,
expires_at). `Authorization: Bearer slt_…`. Auth middleware accepts either the
session cookie/JWT or a valid API token; tokens bypass CSRF (no cookie). Manage
in settings (create shows the secret once; revoke).

**Acceptance criteria:**
1. A created token authenticates API calls; scopes enforced.
2. Token shown once; stored only as a hash; revoke is immediate.
3. `last_used_at` updates; expired tokens reject.

**Test plan:** Integration: token auth path (valid/expired/revoked/scope-denied);
CSRF not required for token requests but still required for cookie requests.
Unit: token generation + hashing + scope check.

---

### `[ ]` F10 — OIDC / SSO
**PR:** _none yet_ · **Depends on:** none · **Size:** L

**Goal:** Log in via an external OIDC provider (Authentik, Keycloak, Google…).

**Design:** Standard OIDC auth-code + PKCE flow; configurable issuer/client.
Map claims → user (create/link on first login); optional domain allowlist;
session issued via the existing session machinery. Keep local login unless
disabled by config.

**Acceptance criteria:**
1. OIDC login creates/links a user and issues a normal session.
2. Domain allowlist enforced; state/nonce/PKCE validated.
3. Local login still works unless explicitly disabled.

**Test plan:** Integration with a mock OIDC issuer: happy path, tampered state
rejected, domain not allowed rejected. Unit: claim→user mapping.

---

### `[ ]` F11 — Two-factor auth (TOTP, WebAuthn later)
**PR:** _none yet_ · **Depends on:** none · **Size:** M

**Goal:** Optional TOTP second factor; recovery codes.

**Design:** `user_totp` (secret, enabled_at), `recovery_codes` (hashed). Enroll
flow (QR + verify), step-up at login when enabled, recovery-code fallback.
Admins can require 2FA org-wide (config).

**Acceptance criteria:**
1. Enroll → subsequent logins require a valid TOTP code.
2. Recovery code logs in and is single-use.
3. Wrong codes are rate-limited (reuses H1).

**Test plan:** Unit: TOTP verify (window tolerance), recovery-code hashing/consume.
Integration: login requires second factor when enabled; recovery path.

---

# M16 — Analytics, data & billing

### `[x]` F13 — Flow metrics
**PR:** #29 (lead time + throughput + WIP; cycle-time = follow-up) · **Depends on:** none · **Size:** M

**Goal:** Cycle time, lead time, throughput, cumulative-flow diagram.

**Design:** Derive from task status-transition history (add a
`task_status_events` log if not already captured). New analytics endpoint +
charts (Recharts). Time windows + per-project scope.

**Acceptance criteria:**
1. Cycle/lead time computed from real transitions; throughput per week.
2. CFD renders status distribution over time.
3. Empty/low-data states use the personality copy.

**Test plan:** Unit: metric math over a synthetic transition series. Integration:
endpoint returns correct aggregates for seeded transitions.

---

### `[ ]` F15 — Backup restore + scheduling + retention
**PR:** _none yet_ · **Depends on:** none · **Size:** M

**Goal:** Close the backup loop: scheduled `pg_dump`, retention, and a restore path.

**Design:** Worker cron-like schedule for `create_backup`; retention policy
(keep N / N days, prune from MinIO + rows). Restore: documented, admin-triggered
`sprintly-api restore <key>` one-shot with strong confirmation guards (never
implicit/automatic).

**Acceptance criteria:**
1. Scheduled backups run on the configured interval; retention prunes old ones.
2. Restore from a chosen backup reproduces the data in a staging DB.
3. Restore requires explicit confirmation and is audit-logged.

**Test plan:** Integration: backup row lifecycle + retention prune logic. Manual:
documented restore drill on a throwaway DB.

---

### `[ ]` F16 — Import / export
**PR:** _none yet_ · **Depends on:** F7 · **Size:** M

**Goal:** Migrate in (Jira/Trello/CSV) and full data export out.

**Design:** Importers mapping external entities → projects/boards/tasks/labels;
dry-run + report. Export: per-project JSON bundle (tasks, comments, attachments
manifest) and CSV.

**Acceptance criteria:**
1. A Trello/CSV sample imports into a project with cards→tasks, lists→columns.
2. Import dry-run reports what would change without writing.
3. Project export round-trips key data.

**Test plan:** Unit: mapping of sample fixtures. Integration: import fixture →
expected rows; export contains expected entities.

---

### `[ ]` F14 — Invoicing / per-client billing
**PR:** _none yet_ · **Depends on:** none · **Size:** M

**Goal:** Turn billable time + rates into client invoices.

**Design:** Build on payroll/time: `clients` + project→client; generate an
invoice (period, line items by project/task, totals) → PDF (reuse `infra::pdf`)
+ CSV; mark sent/paid.

**Acceptance criteria:**
1. Generate an invoice for a client/period from billable logs at configured rates.
2. Invoice PDF totals match the underlying time × rate.
3. Mark sent/paid lifecycle works; admin-only.

**Test plan:** Unit: invoice math (rounding, currency). Integration: seeded
billable logs → expected invoice totals + line items.

---

# M17 — Polish

### `[ ]` F17 — Mobile / responsive + PWA
**PR:** _none yet_ · **Depends on:** none · **Size:** M

**Goal:** Usable on phones; installable PWA with basic offline shell.

**Design:** Responsive board/task/detail layouts; touch-friendly DnD; PWA
manifest + service worker (app shell + read cache); offline indicator.

**Acceptance criteria:**
1. Board + task detail are usable at 375px width.
2. App is installable; the shell loads offline; clear offline state.

**Test plan:** e2e (mobile viewport): board renders + a task opens; Lighthouse
PWA check in CI (optional).

---

### `[ ]` F18 — Public read-only status pages
**PR:** _none yet_ · **Depends on:** none · **Size:** S

**Goal:** Shareable, unauthenticated read-only project status (opt-in per project).

**Design:** Per-project public token; a read-only page showing sprint progress +
selected board columns (no secrets, no vault, no private fields). Off by default.

**Acceptance criteria:**
1. Enabling produces a tokenised public URL showing sprint + board summary.
2. No authenticated/private data leaks; vault never exposed.
3. Disabling invalidates the URL.

**Test plan:** Integration: public endpoint returns only whitelisted fields;
disabled token 404s. e2e: public page renders without a session.

---

## Changelog of this file

- _initial_ — roadmap created with M11–M17 (H1–H7, F1–F18).
