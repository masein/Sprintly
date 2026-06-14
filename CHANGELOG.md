# Changelog

All notable changes to Sprintly are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
