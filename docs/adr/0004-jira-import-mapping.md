# ADR 0004 — Native Jira import field mapping

**Status:** accepted · **Date:** 2026-06-22 · **Scope:** F16 (Jira importer)

**Revision (2026-06-22):** hardened the structural mapping against real exports —
see *Hierarchy* below. Unified epic/sub-task resolution across the classic
`Epic Link` and the team-managed `Parent` model, warn on absent parents, carry
sprint window + state, preserve comment author/date, and remap `Task → feature`.

**Revision (2026-06-23):** faithful historical migration — see *Historical
sprints & user provisioning* below. Imported sprints default to **completed**
(not planned), and an opt-in provisions Sprintly users for unmatched
assignees/reporters with a force-reset temp password.

## Context

F16 shipped Trello-JSON and a minimal CSV importer that kept only
title/description/status/labels. A real Jira migration carries far more —
assignee, priority, issue type, sub-tasks, epics, sprints, story points, and
comments — and a line-based CSV reader can't even survive Jira's multi-line
quoted descriptions. We want a first-class Jira source that maps richly and is
safe to re-run.

The interesting decisions are at the **field-mapping boundary** (Jira's model
is richer and differently-shaped than Sprintly's), so they're recorded here.

## Decision

A dedicated, pure parser (`domain::jira`) turns a Jira **"Export Excel CSV (all
fields)"** export into a typed `JiraPlan`; `import_export::apply_jira_import`
writes it inside the same dry-run-by-rollback transaction the other importers
use. Mapping rules:

- **CSV-first.** We target the Excel-CSV export (the one Jira users actually
  produce). The REST/JSON issue export is **deferred** — same `JiraPlan` model
  would back it later, so it's additive.
- **Robust CSV.** The Jira path uses the `csv` crate (RFC-4180) so newlines
  inside quoted cells survive. The legacy simple-CSV path is left as-is.
- **Repeated headers.** Jira repeats `Labels`, `Comment`, and `Sprint` column
  names; we collect *every* column sharing a name, not just the first.
- **Auto-detect.** A CSV whose header set includes `Issue key` + `Issue Type` +
  `Summary` is treated as Jira even when uploaded as plain `.csv`, so users get
  the rich path without picking a format.
- **Priority** Highest→p0, High→p1, Medium→p2, Low/Lowest→p3 (+ Blocker/Critical
  →p0, Major→p1, Minor/Trivial→p3); unknown → the p2 default.
- **Issue type** Story/Improvement/New Feature→feature, Bug→bug, Spike→spike,
  Incident→incident, Task/Sub-task→chore. **Epics are not tasks** — an
  Epic-type row becomes a Sprintly **epic**, and other issues' `Epic Link`
  associate to it (by Jira key, else by name).
- **Sub-tasks.** An issue with a `Parent` is linked via `parent_task_id` to the
  parent's imported task (resolved in a second pass, since order isn't
  guaranteed).
- **Assignee** matched by **email first, then display name**; an unmatched
  assignee leaves the task unassigned and is **collected into one warning**.
- **Story points → a `Story Points` number custom field** (F7), created if
  absent. We use a custom field rather than the native integer column because
  Jira points are frequently fractional (0.5, 1.5) and the field preserves them.
- **Comments** are imported (author matched, body kept) **only when a task is
  first created** — so a re-import doesn't grow duplicate comment trails.
- **Idempotency.** The Jira **issue key** is stored on `tasks.external_ref`
  (new column, partial-unique per project). A re-import matches by it and
  **updates in place** instead of duplicating.
- **Sprints** are created from distinct Sprint names with a placeholder
  fortnight window and `planned` state — Jira's CSV doesn't reliably carry a
  sprint's dates or open/closed state, so the lead adjusts after import.

## Hierarchy (revision)

A real "all fields" export carries the parent relationship two different ways,
and the first cut only handled one:

- **Classic / company-managed** projects use a separate **`Epic Link`** column
  (the epic's key) plus **`Parent`** for sub-tasks.
- **Team-managed** projects fold everything into one **`Parent`** column: a
  sub-task's `Parent` is its story, but a *story's* `Parent` is its **epic**.

So resolution is unified in a second pass once every task id is known:

- **Epic membership** comes from `Epic Link` **or** a `Parent` that points at an
  epic in the import (matched by Jira key) → `tasks.epic_id` is set, making epic
  progress (done/total) real. An epic parent is *not* treated as task nesting.
- **Sub-task nesting** comes from a `Parent` that points at a non-epic task we
  imported → `tasks.parent_task_id`. If the parent is **absent** from the
  import, the task stays **top-level** and is collected into a warning (never
  silently flattened, never dangling).
- **Comments:** the author is matched to a Sprintly user; when there's no
  account, the original name is preserved in the comment body (`> imported from
  Jira — Name`) rather than dropped, and the Jira timestamp becomes `created_at`.
- **Sprints:** a bare name still defaults to a planned fortnight, but the rich
  GreenHopper `...[state=…,startDate=…,endDate=…]` cell some exports emit is
  parsed for the real **window + state** (`active`/`closed`→`completed`). The
  one-active-sprint constraint is respected — a second "active" imports as
  planned with a warning.
- **Issue type:** a generic **`Task` → `feature`** (Sprintly's neutral default),
  not `chore`; for many teams the Task is the primary work item. `Sub-task` has
  no real sub-type in CSV, so it stays `chore` and gets its link from the pass.

## Historical sprints & user provisioning (revision)

A Jira import is a migration of *finished* work, which changes two defaults:

- **Sprints default to `completed`, not `planned`.** A planned sprint shows a
  "start sprint" button and an empty fortnight — wrong for a sprint that ended a
  year ago. Imported sprints are `completed`, carrying the real `startDate` /
  `endDate` and `completed_at` when the export's Sprint cell encodes them.
  Explicit Jira state is honoured (`ACTIVE`→active, `CLOSED`→completed,
  `FUTURE`→planned). When nothing is marked active, the **most-recent
  *dated*, open-ended** sprint is promoted to active so the in-flight sprint
  stays live; name-only sprints (no dates) are never promoted — they all stay
  completed. The one-active-per-project rule still holds (a second would-be
  active imports as completed + warns).
- **Opt-in user provisioning.** Off by default (match-only + warn, unchanged).
  When on, each unmatched assignee/reporter gets a Sprintly account — display
  name from Jira, a slugged unique handle, the raw value as email if it is one
  else a synthetic `@jira-import.local` address — added to the project as a
  contributor, and their cards assigned (reporter too). Accounts are created
  with an **operator-supplied temp password** (hashed in the route so the
  plaintext never reaches the domain layer or the logs) and a
  **`must_change_password`** flag. Re-import is idempotent: provisioned users
  match by name/email on the next run, so no duplicates.

**Force-reset enforcement.** `must_change_password` is a new (reversible) column.
Login mirrors the 2FA step-up: a flagged account's correct password yields a
short-lived **challenge**, not a session. The user spends it at
`POST /auth/password/change` to set a real password (which clears the flag) and
*then* gets a session. So a provisioned account can't reach anything with the
shared temp password.

## Consequences

- One new nullable column + partial-unique index; no destructive change. Down
  migration drops both.
- The simple Trello/CSV importer is untouched; its report gains zeroed
  Jira-only fields (`epics_created`, `tasks_updated`, …) for a uniform shape.
- A future Jira-JSON source slots in behind the same `JiraPlan`/apply path.
