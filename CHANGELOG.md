# Changelog

All notable changes to Sprintly are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **The project key is renamable** — a pencil next to the key in the project header (leads only). Renaming rewrites **every task key in the project** (`TST-12` → `OPS-12`) in one transaction — soft-deleted tasks included, notification links repointed, and the task-number sequence keeps counting. The UI warns loudly first: old URLs, bookmarks, and keys written into commit messages stop resolving; that's the price of changing a project's identity, and it's why the confirm is wordy. Concurrent task creation can't mint a stale prefix (the rename holds the same row lock the key sequence uses). Reported as "Project key cannot be modified."

- **Points, due date, and estimate are editable on the task detail** — the schema and API carried `story_points`, `due_date`, and `estimate_minutes` since day one, but no UI ever set them (they only appeared read-only when an import filled them in). The details panel now has editors for all three — points feed sprint totals/velocity, due dates feed the dashboards' overdue/upcoming lists, estimate is entered in hours, and all three are clearable (the PATCH API now uses the same explicit-null contract as assignee, instead of COALESCE silently ignoring nulls). Reported as "Estimate and Due Date fields are missing."

- **Tasks and subtasks convert into each other** — the `parent` row in a task's details grows up: `make subtask of…` tucks an existing task under a parent (searchable picker), `↑ promote` turns a subtask back into a top-level task, and `move…` shifts a subtask to a different parent. New `PUT /tasks/:key/parent` endpoint with the guardrails the bare column never had: same-project only, one level deep (no nesting under a subtask), no demoting a task that has subtasks of its own — and demotion drops the task from its sprint so it isn't counted twice (subtask time/points roll up under the parent). Until now `parent_task_id` could only ever be set at creation. Reversible migration widens the activity-feed `kind` CHECK for the new `reparented` entries.
- **The backlog lives on the sprint page — drag across** — the sprint page gains a `backlog` panel next to the task list: drag a backlog row (by its grip) onto the task list to commit it to the sprint, drag a sprint row onto the panel to send it back. The per-row buttons (`+ add tasks`, the remove icon, the task detail's sprint select) all remain — drag is a shortcut, not the only door. Panel and grips disappear once the sprint completes.
- **Epics can change color after creation** — the color swatch on an epic's row (timeline page) is now a picker for leads: click it, choose a new swatch, and the bar follows. The API accepted `color` on epic updates all along; the UI just never offered it — color was a creation-time-only choice.
- **`← board` on the sprint page** — the sprint detail now links straight back to the project board, the same affordance the backlog page has. Before, getting from a sprint to its board took a detour through the sprints list or the project switcher.
- **The retro summary is editable after closing** — closing a retro generates deterministic markdown from the notes; it's a starting point, not the final word. Leads (and admins) get an `edit` button on the summary right on the retro page — rework it until it reads like a human wrote it (`PATCH /sprints/:id` now accepts `summary_md`, only once the sprint is completed; meta edits on completed sprints stay refused).
- **Retro notes are editable** — your own (non-anonymous) notes get a pencil while the retro is open: edit in place, with an `· edited` marker once changed (admins can edit any note, including anonymous ones). The edit endpoint existed but no UI called it, and it skipped the checks its siblings had — it now requires project access and an open retro (a closed retro's summary already snapshotted the notes, so late edits would make the record lie). Authors can also delete their own notes from the UI now, not just admins — a permission the API always granted.
- **Parent tasks count their subtasks' time** — the timer panel on a task now shows a `tracked` total that includes time logged on its direct subtasks (`tracked 3h · 1h in subtasks`), via a new `GET /tasks/:key/time-summary` endpoint. Before, subtask time was invisible from the parent — the total tracked time of a broken-down task was scattered across its children.
- **Historical time logs on My Day and the dashboard** — both surfaces were stuck on the current week. My Day gains a `clockwork` panel (week total, per-day bars, top tasks) and the project dashboard's `top contributors` panel gains the same `‹ ›` week navigation — step back through any past week, with a one-click `this week` reset. Powered by the existing per-week timesheet and range time-report endpoints; on the dashboard, non-leads see their own logs only (the server's existing scope rule, now labelled honestly).

- **@mentions you can actually type** — the notification plumbing for mentions existed since M12, but nothing helped you write one and nothing showed one. Now: comment boxes and the task description editor autocomplete `@` against the project's members (↑/↓/Enter or click to pick); rendered markdown highlights `@handle` (code spans, links, and emails stay plain); and mentioning someone in a task description finally notifies them — on create, and on edit only the handles *newly added* to the text, so re-saving a description never re-pings the people already in it.
- **Sprint field on the task detail** — the details panel gains a `sprint` select: pick `backlog · none` to pull a task out of its sprint, or move it straight into another one. Until now the only way out of a sprint was the small remove icon on the sprint page itself — reported from real usage as "move a sprint task to backlog: not found".
- **Admin invites tab + email editing** — `/admin` gains an `invites` tab for the long-existing invite API: mint a one-shot signup link with a role attached (member / admin / viewer — an admin invite makes an admin), optionally emailing it, with the link shown once, a list of outstanding/used/expired invites, and revoke. The minted `/register?invite=…` link now **prefills the token** on the register form instead of making the invitee fish it out of the URL. The users tab gains **inline email editing** (new admin-only `POST /admin/users/:id/email`, validated, citext-duplicate → friendly 409, audit-logged) — the user signs in with the new address immediately.
- **Sprint swimlanes on the board** — the swimlanes control gains a `sprint` grouping: the active sprint lands in the top lane, any other sprints with cards in view sit below it (most recent first), and the backlog / no-sprint pile comes last — so under `all tasks` scope you can tell committed work apart from everything else. Saveable as a board view like the other groupings. The task list now carries `sprint_id` so the board can group without extra round-trips (widened the `board_views.group_by` CHECK to allow `sprint`).
- **Project members UI** — a `members` section on the project page (a chip next to `labels`/`fields`) finally exposes the long-existing members API in the web app: it lists everyone with their avatar, handle, and role. Leads can add an existing user (typeahead by handle **or** email), change a role (lead / contributor / watcher), and remove someone (with confirmation, and no "delete" language — they can be re-added). Non-leads see the same list read-only. API errors are surfaced verbatim (e.g. the server's refusal to remove the last lead). User search now also matches an email prefix (the address is never returned) so you can find someone to add by their email.
- **Backlog quick-add** — the backlog page ("The pile.") now has an inline `+ add a task` row: type a title and Enter files a sprint-less task into the default board's first column, Esc collapses, and the list updates without a reload. It stays open and refocuses after each add for rapid entry, and an empty submit gets an inline nudge instead of silently doing nothing.
- **Jira worklog import** — the native Jira importer now maps the repeated `Log Work` columns (`comment;started;author;timeSpentSeconds`) into `time_logs`: author matched like comments and assignees (unmatched → skipped with a warning, never invents a user), start time and duration preserved. Idempotent on re-import (deduped on task + user + start + duration); the dry-run preview reports the worklog count that would land.
- **Air-gapped production deployment** — a self-contained `infra/compose/docker-compose.prod.yml` where every image is pulled from a private registry (`REGISTRY_HOST`), only the reverse-proxy (web) port is published, all other services stay internal, and secrets are read fail-fast from `.env`. New `release.yml` workflow builds the `sprintly-api`/`sprintly-web` images (`linux/amd64`) on merge to `main`, tags `latest` + `sha-<short>`, and pushes to the registry with retries. Runbook in [`docs/RUNBOOK-PROD.md`](docs/RUNBOOK-PROD.md) covers mirroring base images, `scp`/`.env` bootstrap on Alpine (`/dev/urandom`, no `openssl`), and the `docker compose pull && up -d` deploy flow.

### Fixed

- **Membership and role changes are live** — adding someone, changing their project role, or removing them now publishes a `member_changed` WebSocket event; open sessions refresh the project, its member list, and the projects index on the spot (reported: "Changing a member role requires a page refresh before taking effect"). The event reaches the affected user even when they were just removed. This also dissolves the reported "contributors can promote themselves to Lead": the server always refused (verified — every path 403s); what QA saw was a stale panel after a role change pretending the change worked.
- **Manual time entry stays inside its card** — the date · hours · minutes · billable row didn't wrap, so in the ~280px task sidebar it poked out of the timer card (reported with a screenshot: "Add time log appears to be partially out of the box"). The row wraps now.
- **Sprint page: a very long task title pushed the sidebar off-viewport** — grid children default to `min-width: auto`, so one unbreakable title widened the task column past the viewport and the backlog/burndown column got clipped. Bounding the column (`min-w-0`) lets the existing row-level truncation do its job.
- **Logout actually leaves** — the logout button revoked the session but then stayed on the current page with every query cache intact, so the app kept showing signed-in data until a manual refresh (reported: "Logout redirection … requires manual page refreshes"). It now clears the client caches and lands on `/login`. Both logout buttons (inline header + mobile menu) fixed.
- **409s and 400s say what actually went wrong** — the error layer flattened every conflict to `That already exists.` and every bad request to `That request didn't parse.`, throwing away the hand-written explanations the handlers produce. Deleting a non-empty column now says `column still has tasks — move them first` (reported: it claimed "That already exists."), timer clashes say `you already have a running timer — stop it first`, and so on across ~25 call sites. 5xx and auth errors keep their deliberately generic copy.
- **Admin password reset actually works** — broken twice over: the minted link pointed at `/login?reset=…`, a parameter the login page never read (the real page is `/reset?token=…`), and the panel then tried to copy it with `navigator.clipboard`, which doesn't exist on plain-http deployments — crashing before the admin ever saw the URL. The link now points at the reset page, and the admin panel shows it in a copyable field (select-on-focus + a fallback-backed copy button) with its expiry, instead of clipboard-and-pray.
- **Copy buttons work on plain-HTTP deployments** — every copy button in the app (retro summary, invite links, webhook secrets, API tokens, 2FA recovery codes, vault reveals, public status links) called `navigator.clipboard`, which only exists on https/localhost — on a bare-IP http deployment they all silently failed (reported: "Copy Summary in Retrospective is not functioning"). A shared helper now falls back to the legacy textarea trick and reports honestly whether the copy landed.

- **Admins can manage any project's members from the UI** — the members panel showed global admins the read-only view unless they also happened to be a project lead, even though the API always allowed them to add members, change project roles, and remove people (admin short-circuits permission checks). The panel's manage controls now appear for global admins on every project. Reported from real usage: "adding a member — I couldn't find it."
- **Realtime was dead behind the reverse proxy — every page updates live now** — two independent bugs, either fatal alone. (1) The WebSocket route was registered under `/api/v1/ws` while both Caddyfiles proxy `/ws`; the socket now also lives at bare `/ws` (the `/api/v1/ws` alias stays). (2) The Caddyfiles proxied `/ws` with a top-level `reverse_proxy @ws` — but Caddy runs `handle` directives first, so the frontend catch-all `handle` swallowed `/ws` (→ web → 404) and that line was dead code; the ws proxy now lives in its own `handle` block (dev + prod). Net effect before the fix: the app never received a single push event behind the proxy — no live board moves, no live notification bell, no presence — everything silently degraded to polling. This is what surfaced the reported "My Day needs a manual refresh".
- **My Day updates itself** — beyond the dead socket, the page only polled once a minute (and not at all while the tab was in the background), never refetched on tab focus, and no WebSocket event ever touched its query. Task events over the WebSocket now invalidate the My Day, project-dashboard, and my-tasks queries (notifications refresh My Day too, for the watched list), and My Day also refetches on window focus. The 60s poll stays as the fallback.

- **Cards couldn't be dragged into another column in swimlane views** — with any grouping active (assignee / label / priority / sprint), the drop zones inside a lane carry a lane-suffixed id that the drag handler didn't recognise, so dropping a card into a column's body was silently ignored — a move only landed if you happened to drop exactly on top of another card. Column-body drops now work in grouped views the same as on the plain board.
- **Attachments 403'd behind a path-based MinIO proxy** — the presigner signed the SigV4 host as `host:port/s3` (the public endpoint's path wasn't stripped), while MinIO verifies against the bare `Host: host:port` header — so on deployments where `MINIO_PUBLIC_ENDPOINT` routes through the reverse proxy (the documented prod setup), every presigned upload and download was rejected with `SignatureDoesNotMatch`. The host is now stripped to host:port; dev and CI route attachments through the same `/s3` proxy as prod so this topology stays covered (new upload/download e2e).
- **Failed loads no longer masquerade as loading (or as empty)** — when a page's data fetch failed for any reason other than the specially-handled 401/403/404, most pages either sat on the loading placeholder forever ("compiling vibes…" with nothing compiling) or rendered a misleading empty state ("Backlog zero" on a backlog that simply didn't load). Every data-driven page (dashboard, my day, sprint, retro, timeline, vault, backlog, metrics, my tasks, achievements, timesheets, approvals, payroll, billing) now shows an honest error box with what the server said and a `$ retry` button that refetches in place.
- **Quick-add under a sprint scope created sprint-less cards** — when the board was scoped to a sprint (active or a pinned one), a column's `+ add card` created the task with no sprint, so it immediately dropped out of the filtered view. The quick-add now inherits the board's scope: a card added while scoped to a sprint joins that sprint and stays visible; under "all tasks" scope it stays a sprint-less backlog card as before. In swimlane mode the sprint is merged on top of the lane's assignee/label/priority defaults.
- **Board scope didn't reliably default to the active sprint** — the scope choice was persisted to `localStorage` per project, so switching to "all tasks" once made every later open stay on "all tasks" — a running sprint never reclaimed the focus. Scope is no longer persisted across loads: a running sprint wins on every fresh open (matching how a sprint board is expected to behave), switching scope is a session-only move, and a reload snaps back to the active sprint. With no sprint running the board opens on "all tasks" as before. Saved views are unaffected (they never pinned a scope).
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
