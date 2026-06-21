# ADR 0004 — Native Jira import field mapping

**Status:** accepted · **Date:** 2026-06-22 · **Scope:** F16 (Jira importer)

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

## Consequences

- One new nullable column + partial-unique index; no destructive change. Down
  migration drops both.
- The simple Trello/CSV importer is untouched; its report gains zeroed
  Jira-only fields (`epics_created`, `tasks_updated`, …) for a uniform shape.
- A future Jira-JSON source slots in behind the same `JiraPlan`/apply path.
