# Sprintly

[![CI](https://github.com/masein/Sprintly/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/masein/Sprintly/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/masein/Sprintly/branch/main/graph/badge.svg)](https://codecov.io/gh/masein/Sprintly)

Self-hosted, dev-themed project management. Kanban-first, with sprints, time
tracking, payroll, and an encrypted per-project secrets vault. Built for small
software teams who'd rather use a thing than configure a thing.

> **Coverage:** the badge reflects **unit + integration** line coverage. The
> backend's domain layer and the core HTTP routes (auth, projects, boards,
> tasks, sprints) are integration-tested against a real Postgres; the rest of
> the **web UI** and the remaining routes are exercised by the Playwright
> **e2e** suite, which runs against an un-instrumented build — so that part
> isn't counted here and the number understates real testing. See the `api` /
> `web` flags on Codecov for the split.

> Codename in the repo is **Sprintly**. The marketing name may change later;
> the codename stays.

---

## Stack (locked)

- **Backend:** Rust (stable, edition 2021), Axum, SQLx, PostgreSQL 16, Redis 7
- **Frontend:** Next.js 14 (App Router), TypeScript strict, Tailwind, shadcn/ui
- **Storage:** MinIO (S3-compatible)
- **Reverse proxy:** Caddy (auto-HTTPS in prod)
- **Everything ships in Docker.** There is no other supported way to run it.

Full architecture rationale: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

---

## Quickstart (dev)

You need: Docker, `just` (`brew install just` / `cargo install just`).

```bash
git clone <repo>
cd sprintly
cp .env.example .env
just up
```

Within a minute or two:

| Service | URL                                  |
| ------- | ------------------------------------ |
| Web     | http://localhost:8080                |
| API     | http://localhost:8080/api/v1/healthz |
| MinIO   | http://localhost:9001 (console)      |

Tail logs with `just logs`. See everything `just` can do with `just`.

If the API container exits immediately, the env is usually the culprit. Run
`docker compose ... exec api sprintly-api check-config` (or `sprintly-api
check-config` locally) to validate every variable and print a redacted summary
— it names the offending variable instead of failing silently.

### Tests & lint

With the stack up (`just up`), run the suites against it — no host toolchain
needed. These spin up one-shot `tools`/`node` containers wired to the running
Postgres/Redis (the `runtime` images ship no compiler):

```bash
just test          # full backend suite (cargo test --workspace)
just lint          # cargo clippy -D warnings + cargo fmt --check + web eslint
just test-web      # web typecheck
just sqlx-prepare  # regenerate apps/api/.sqlx after changing a query! macro
```

CI runs the same checks on every PR.

---

## Repo layout

```
sprintly/
├── apps/
│   ├── api/                 # Rust / Axum backend
│   └── web/                 # Next.js frontend
├── packages/
│   └── shared-types/        # TS types generated from backend OpenAPI
├── infra/
│   ├── docker/              # Dockerfiles + Caddy config
│   └── compose/             # docker-compose files
├── scripts/
├── docs/
├── .github/workflows/       # CI
├── justfile
├── .env.example
└── README.md
```

---

## Milestones

Built in order. Each one ships fully usable before the next begins.

- [x] **M1** — Skeleton, auth, users, settings ✓
  - phase 1: workspaces + compose stack + healthchecks
  - phase 2: schema, argon2id, JWT + rotating refresh w/ reuse detection,
    RBAC `can()`, register/login/logout/refresh/password-reset, `/users/me`
  - phase 3: admin invite tokens, `/settings` page, demo seed, Playwright smoke
- [x] **M2** — Projects, boards, columns ✓
  - Projects (key/icon/color/archive) + per-project members (lead/contributor/watcher)
  - Default Kanban board auto-provisioned on create with three columns
  - Column CRUD with category, WIP limits, fractional drag-reorder (dnd-kit)
  - Project switcher in top bar, /projects list + create modal, project home
  - Project-scoped permissions; archived projects refuse writes
  - CSRF nonce (double-submit) on browser-origin writes
- [x] **M3** — Tasks & Kanban ✓
  - **A:** tasks schema (key sequencer, tsvector, GIN indexes), CRUD + move endpoints,
    `/ws` Redis pub/sub fan-out, TanStack Query on the web, card render + DnD,
    optimistic moves with rollback ✓
  - **B:** task_comments / task_reactions / task_attachments schema, MinIO
    SigV4 presigned URLs (hand-rolled, no aws-sdk dep), full /tasks/:key
    detail page with markdown (react-markdown + remark-gfm + rehype-sanitize),
    threaded comments + emoji reactions, activity feed, watchers, two-phase
    attachment upload with XHR progress ✓
  - **C:** cross-project `/search` (tsvector + pg_trgm) + `/me/tasks`, cmd-K
    command palette w/ task search + actions + nav, global keyboard hotkeys
    (`/` `?` `c` `g p` `g m` `g s`), board filter chips, subtasks + links
    panels on the task detail, konami → CRT mode, `sudo` / `rm -rf` /
    `:q` / `:wq` easter eggs ✓
- [x] **M4** — Estimation & time tracking ✓
  - `time_logs` (generated `duration_minutes` column, partial-unique
    "one running per user" index) + `timesheets` (per-week, immutable
    snapshot on submit)
  - Start/stop timer + manual entry, header-pinned running-timer chip with
    live mm:ss counter, per-task log list
  - Weekly timesheet view (7-day grid + by-task), submit → approve flow,
    CSV export, approval lock on logs in the approved range
  - `/timesheets` approval queue scoped to global admins + project leads
    whose members logged time
- [x] **M5** — Sprints & retros ✓
  - `sprints` with state machine (planned → active → completed) + partial-
    unique "one active per project" index. Deferred FK on `tasks.sprint_id`
    finally wired.
  - Sprint CRUD, start/complete, task assignment, burndown endpoint that
    computes a 7-day series (actual stepped + ideal) on the fly.
  - `sprint_retros` 1-1 per sprint, auto-opened on complete. Velocity
    snapshotted at completion.
  - Retros: 4 columns, anonymous notes (author_id = NULL), voting, one-
    click promote action item → task, close → generates a sharable markdown
    summary stored on the sprint.
  - Recharts burndown chart on the frontend, copy-to-clipboard summary.
- [x] **M6** — Dashboards ✓
  - `GET /projects/:key/dashboard` — single-call aggregate: status counts,
    current sprint with burndown, velocity history (last 10 closed),
    top contributors this week, recent activity (20), blocked tasks,
    upcoming due dates (14d), time this week.
  - `GET /me/dashboard` — personal "My day": my status counts, overdue,
    next-up sample, watched changed in last 7d, running timer ref,
    time this week.
  - Project dashboard page (`/projects/[key]/dashboard`) with Recharts
    velocity bars + reused burndown.
  - Personal dashboard page (`/me/day`) linked from session badge.
  - `g d` chord added; `?` help sheet updated.
- [x] **M7** — Vault ✓
  - HKDF-SHA256 per-project key derivation from a 32-byte master in env;
    XChaCha20-Poly1305 AEAD with a 24-byte nonce per write; AAD bound to
    item id. Wrong key / tampered ct / tampered nonce / tampered AAD all
    fail loud.
  - Schema CHECK on nonce length (24). `vault_audit_log` is **append-only**
    via triggers that block UPDATE and DELETE.
  - Reveal endpoint: rate-limited via Redis token bucket (10/hour/user),
    audit-logged, never leaks plaintext on failure paths.
  - Frontend: per-project /vault page with categorised list. Click-to-reveal
    holds plaintext only in component-local state with a 10s countdown;
    React unmount = wipe. Copy uses Clipboard API with a 30s auto-clear.
    Audit-log + access drawers per item.
  - `ConnectInfo<SocketAddr>` wired into the server bootstrap so audit
    rows carry an `ip` field.
- [x] **M8** — Payroll ✓
  - `projects.budget_cents` + `projects.budget_currency` columns;
    `payroll_periods` table keyed (user_id, year, month) for paid-status
    bookkeeping independent of weekly timesheet rows.
  - Monthly aggregation in SQL: per-user totals + per-project breakdown,
    billable filter, pay = billable × hourly_rate / 60 in cents.
  - `GET /payroll/:year/:month` + `.csv` + `/payroll/:user/:year/:month` +
    `.pdf` + mark-paid / reopen endpoints. Admin-only.
  - **Hand-rolled PDF** (`infra::pdf`) — ~80 lines, no PDF crate. Built-in
    Helvetica, US Letter, valid PDF 1.4. Avoids the multi-MB dep.
  - `PATCH /projects/:key/budget` + `GET /projects/:key/burn` (current-month
    spend vs budget with elapsed-fraction ratio).
  - Frontend: `/payroll` admin page with month nav, CSV + per-row PDF.
    `BurnWidget` on project dashboard with inline budget editor.
- [x] **M9** — Personality & polish ✓
  - `achievements` + `user_achievements` tables seeded with the catalog of 8.
  - **Background worker** (Tokio task on boot) polls `jobs` with FOR UPDATE
    SKIP LOCKED and re-enqueues `scan_achievements` every 5 min. Exponential
    backoff on failures.
  - Rules in `domain::achievements` (one SQL query per code). `RTFM`
    triggered immediately when the user opens `/docs`.
  - **Themes**: 5 named themes via CSS variables (`midnight`, `daylight`,
    `solarized_dusk`, `terminal_green`, `hot_pink`). Setting persists in
    localStorage; `terminal_green` swaps the whole body to JetBrains Mono.
  - **Sprint** mascot: pixel-art SVG, 7 moods, April-1st sunglasses.
    Adopted in empty states + landing + docs sidebar.
  - **CoffeeMeter** chip in the header; **AchievementToast** with confetti
    fan-out on award; confetti also fires on sprint complete.
  - In-app docs at `/docs` (RTFM trigger), `/me/achievements` page with
    earned + locked rows, palette actions for both.
- [x] **M10** — Admin & ops ✓
  - `admin_audit_log` (append-only trigger), `webhooks` (scaffold,
    delivery deferred), `backups` (pg_dump lifecycle row).
  - X-Forwarded-For aware `middleware::client_ip` resolver — vault audit +
    admin audit now record the real client IP through Caddy.
  - `/admin/users` + suspend / reactivate / role-change / single-use
    password-reset URL; **revoking sessions on suspend** so the user is
    logged out immediately.
  - `/admin/audit` feed, `/admin/health` (DB / Redis / MinIO ping + version
    + job stats), `/admin/backups` POST/GET.
  - **`create_backup` job** in the worker shells out to `pg_dump`, uploads
    via the existing SigV4 signer + `curl`. Runtime image now ships
    `postgresql-client` + `curl`.
  - `/projects/:key/webhooks` CRUD (secret stored hashed).
  - Frontend `/admin` page with five tabs: users / audit / health /
    backups / webhooks. Admin link in cmd-K palette.

Demo credentials after `just seed`: `demo@sprintly.local` / `sprintly`.

---

## Docs

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — system overview
- [`docs/DATA_MODEL.md`](docs/DATA_MODEL.md) — schema + indexes
- [`docs/SECURITY.md`](docs/SECURITY.md) — threat model, vault crypto, RBAC
- [`docs/PERSONALITY.md`](docs/PERSONALITY.md) — voice & style guide

---

## License

TBD. Don't redistribute yet.
