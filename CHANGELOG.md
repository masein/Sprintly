# Changelog

All notable changes to Sprintly are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Jira worklog import** — the native Jira importer now maps the repeated `Log Work` columns (`comment;started;author;timeSpentSeconds`) into `time_logs`: author matched like comments and assignees (unmatched → skipped with a warning, never invents a user), start time and duration preserved. Idempotent on re-import (deduped on task + user + start + duration); the dry-run preview reports the worklog count that would land.
- **Air-gapped production deployment** — a self-contained `infra/compose/docker-compose.prod.yml` where every image is pulled from a private registry (`REGISTRY_HOST`), only the reverse-proxy (web) port is published, all other services stay internal, and secrets are read fail-fast from `.env`. New `release.yml` workflow builds the `sprintly-api`/`sprintly-web` images (`linux/amd64`) on merge to `main`, tags `latest` + `sha-<short>`, and pushes to the registry with retries. Runbook in [`docs/RUNBOOK-PROD.md`](docs/RUNBOOK-PROD.md) covers mirroring base images, `scp`/`.env` bootstrap on Alpine (`/dev/urandom`, no `openssl`), and the `docker compose pull && up -d` deploy flow.

### Fixed

- **Prod Caddy missing a route to MinIO** — `Caddyfile.prod` had no `/s3` proxy, so the `MINIO_PUBLIC_ENDPOINT=.../s3` value documented in `.env.prod.example` pointed nowhere and every presigned attachment URL 404'd. Added a `handle_path /s3/*` route (prefix stripped, matching what the API signs) to `minio:9000`.
- **Responsive pass on the app shell and project surfaces** — a manual audit at 390/768/1280px found the global header overflowing below ~900px (the project-switcher chip wrapping into a floating box, session actions running off-screen and unreachable), page-level horizontal scroll on most pages, a starved project-rename input showing only the tail of the name, wrapping breadcrumbs/task-key badges, and cramped 2-column stat grids on mobile. Fixed: the header collapses session actions (avatar/role/"my day"/settings/logout) into a menu below `lg`, keeping the timer and notification bell always visible, and the switcher chip now truncates instead of wrapping; the project page's title/breadcrumb row is decoupled from its nav-chip row so neither starves the other, and the rename input comfortably shows names well past 60 characters; task-key badges get `whitespace-nowrap` and long titles truncate with a `title` tooltip (backlog, sprint task list); dashboard/my-day stat tiles stack to one column below `sm`; the metrics, dashboard, timesheet, and payroll page headers stack instead of forcing their nav controls to wrap mid-word. Playwright regression suite at 390×844 and 768×1024 covers all of the above.
- **Modals with no height bound** — every dialog in the app (`ImportExportModal`, `CreateProjectModal`, `GitIntegrationsManager`, `FieldsManager`, `LabelsManager`, `PublicStatusModal`, `TemplatesManager`) could grow past the viewport with no way to see or reach the rest of its content — a big Jira import preview (hundreds of report lines) was the case that surfaced it live. `WebhooksManager` already had the right pattern (`max-h-[90vh] overflow-y-auto` on the panel); applied it to the rest so long content scrolls inside the modal instead of spilling off-screen.

### Changed

- **Migrations run at API startup** — `sprintly-api` (serve) now applies pending SQLx migrations before binding, controlled by `SPRINTLY_AUTO_MIGRATE` (default `true`); idempotent, so a redeploy or restart converges the schema with no separate step.
- **Host-agnostic web image** — the WebSocket URL is resolved from the page origin at runtime (falling back to same-origin `/ws`) and `NEXT_PUBLIC_*` default to relative paths, so one CI-built web image works behind any host/scheme without a rebuild.

## [1.0.0] — 2026-06-14

First stable release. Sprintly is a self-hosted, developer-themed project
management tool for small software teams. This release completes the entire
M11–M17 roadmap — 25 planned items (H1–H7, F1–F18) shipped across more than 30
pull requests, each with reversible migrations, integration/e2e tests, and
green required CI.

### Hardening (M11)

- **Auth rate limiting** — token-bucket throttling on login and password-reset endpoints (HTTP 429 + `Retry-After`). (#4)
- **Fail-loud config + secret hygiene** — visible `Config::from_env()` errors, whitespace-tolerant base64, and a `check-config` subcommand. (#5)
- **Reproducible web builds** — committed `pnpm-lock.yaml`; frozen-lockfile installs and CI caching. (#6)
- **Working test/lint recipes** — `just test` / `lint` / `sqlx-prepare` run against the dev stack. (#7)
- **CI hardening** — `sqlx prepare --check`, dependency audit, Dependabot, and a Playwright e2e job. (#8)
- **Correct MSRV** — `rust-version` set to 1.88 to match locked dependencies. (#12)
- **Slim `migrate`** — the subcommand needs only `DATABASE_URL`. (#23)

### Communications (M12)

- **Transactional email** — SMTP (lettre) with a dev log sender for password resets and invites. (#24)
- **In-app notifications** — `@mentions`, assignment and watched-task fan-out, live unread count over WebSocket. (#25)
- **Outbound webhooks + chat** — signed, retried delivery; Slack and Discord adapters; per-project admin UI with test/deliveries. (#26, #50, #51)

### Dev integration ⭐ (M13)

- **Git provider integration** — GitHub / GitLab / Gitea inbound webhooks; branch, commit, and PR → task linking; auto-transition on merge; outbound commit status; `PR_WIZARD` recomputed from real merged PRs. Provider abstraction chosen in ADR 0001. (#27, #28, #46, #47, #48)
- **CI/CD status on tasks & PRs** — a pass/fail/pending chip (icon + label, never colour-only) driven by check and pipeline webhooks. (#49)

### Planning & views (M14)

- **Labels + custom fields** — per-project label registry with colours; text/number/select/date fields; board filter and search integration. (#40, #44)
- **Saved board views + swimlanes** — named, shareable views with grouping by assignee, label, or priority. (#53)
- **Roadmap / timeline** — epics and milestones with a Gantt-lite view and done/total progress. (#54)
- **Templates, recurrence & backlog ops** — task templates, worker-materialised recurrence, and multi-select backlog bulk actions. (#55)

### Identity (M15)

- **Personal API tokens** — scoped bearer auth with shown-once secrets and immediate revoke. (#45)
- **Two-factor auth** — TOTP enrolment with single-use recovery codes; rate-limited. (#56)
- **OIDC / SSO** — auth-code + PKCE; claim→user mapping; optional domain allowlist; local login preserved. External-IdP boundary recorded in ADR 0003. (#57)

### Analytics, data & billing (M16)

- **Flow metrics** — cycle time, lead time, throughput, and a cumulative-flow diagram. (#29, #52)
- **Backups** — scheduled `pg_dump`, retention pruning, and a guarded admin-only restore. (#58)
- **Invoicing** — per-client billing from billable time × rates to PDF/CSV with a sent/paid lifecycle. (#59)
- **Import / export** — Trello/CSV import with dry-run report; per-project JSON and CSV export. (#60)

### Polish (M17)

- **Public status pages** — opt-in, tokenised, read-only sprint and board summary with no private data. (#61)
- **Mobile / PWA** — responsive board and task detail at 375px; installable PWA with an offline shell. (#62)

### Notes

- Every merged item met the global Definition of Done: acceptance criteria, tests green locally and in CI, `cargo fmt`/`clippy -D warnings`/`test --workspace`, `pnpm lint`/`typecheck`/`build`, reversible up/down migrations, regenerated `.sqlx` where queries changed, `/docs` updated, and personality-compliant copy.
- Architecture decisions are recorded in `docs/adr/`: ADR 0001 (git provider abstraction), ADR 0002 (chat webhook adapters), and ADR 0003 (OIDC external-IdP boundary).
- Known advisory: the non-blocking `audit` job reports pre-existing transitive RUSTSEC advisories (idna / rsa / proc-macro-error). No required check is affected.

[1.0.0]: https://github.com/masein/Sprintly/releases/tag/v1.0.0
