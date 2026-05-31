# Data model

> Updated as tables land. Current state: M1 phase 2 is in. The full ER
> diagram (Mermaid) and the complete index list will live here once the
> graph stops thrashing.

## In this phase

```
users ──┬─< sessions ──< refresh_tokens
        ├─< password_reset_tokens
        ├─< invite_tokens (consumed_by)
        ├─< project_members >── projects
        └─< (created_by) projects ──< boards ──< board_columns
```

### Projects (M2)

- `projects.key` is uppercase, 2–10 chars, instance-unique. Used in task IDs
  (`WEB-142`). `CHECK` constraint enforces format at the DB layer.
- One project ⇒ many boards ⇒ many `board_columns`. A project is created with
  one default board (`is_default = true`) and three columns: To do · In
  progress · Done. The "at most one default board per project" rule is a
  partial unique index, not a trigger.
- `board_columns.sort_order` is a `DOUBLE PRECISION`. Inserts use
  `(prev + next) / 2`; the reorder endpoint rebalances to 1024, 2048, 3072, …
  on every call so drift never accumulates.
- `board_columns.category` ∈ {`todo`, `in_progress`, `review`, `done`}. The
  visible `name` is whatever the team wants ("Yeeted to QA"); analytics keys
  off `category`.
- `project_members` is composite-PK (project_id, user_id). Three roles:
  `lead`, `contributor`, `watcher`. Global admins bypass project-role checks.

### Tasks (M3-A)

- **Per-project key sequence.** `projects.next_task_seq` is a `BIGINT`
  incremented inside `UPDATE … RETURNING` whenever a task is created. Atomic
  by virtue of being inside the same transaction as the INSERT. No advisory
  locks needed; concurrent task creates serialize on the project row.
- **`tasks.key`** is `PROJ-N` (e.g. `WEB-142`). Unique per project via a
  partial unique index on `(project_id, key) WHERE deleted_at IS NULL`.
- **Ordering.** `order_in_column` is `DOUBLE PRECISION`. Moves use
  `(prev + next) / 2`. Float precision lasts ~52 inserts between two cards;
  if we ever drift that far we rebalance the column.
- **Status mirrors column category.** Move endpoint copies
  `board_columns.category` → `tasks.status` so analytics queries off
  `status` without joining.
- **`task_activity`** is append-only. Source of truth for the detail page's
  activity feed. The list of `kind` values is closed by a `CHECK` constraint.
- **Search.** `tasks.search_tsv` is a tsvector recomputed by a trigger on
  insert/update of `title`, `description`, `labels`, or `key`. Plus a
  `pg_trgm` GIN index on `tasks.title` for fuzzy match. Search endpoint
  lands in M3-C.
- **`jobs` table** is in place but the worker isn't running yet — kept the
  schema close to where the first jobs will live (achievement awarding,
  search reindex if we ever need it).

- `users.email` is `citext` with `users_email_active_idx` (unique where
  `deleted_at IS NULL`) — soft-deleted accounts free their email.
- `users.handle` mirrors that pattern for @mentions.
- `refresh_tokens.rotated_to` is the rotation chain pointer; reuse
  detection looks for a presented token whose `rotated_to IS NOT NULL`.
- All token tables index by hash (`UNIQUE`) so a presented secret can be
  looked up in O(1) without scans.

## Conventions

- **IDs:** UUIDv7 everywhere. Time-sortable; safe to expose; safe as primary
  keys.
- **Timestamps:** `TIMESTAMPTZ`. Every table gets `created_at`, `updated_at`,
  and `deleted_at` (nullable) unless explicitly noted.
- **Soft delete:** default. Hard deletes are the exception, and they go
  through a documented admin path.
- **JSONB for sparse / extension fields:** `settings`, `custom_fields`,
  `payload` on activity rows.
- **`citext` for email** to dodge case-sensitivity bugs forever.

## Tables (planned, see spec §4)

Core: `users`, `sessions`, `refresh_tokens`, `projects`, `project_members`,
`boards`, `columns`, `tasks`, `task_watchers`, `task_comments`,
`task_reactions`, `task_attachments`, `task_activity`, `task_links`, `sprints`,
`sprint_retros`, `retro_notes`, `retro_votes`, `time_logs`, `timesheets`,
`vault_items`, `vault_access`, `vault_audit_log`, `notifications`, `webhooks`,
`achievements`, `user_achievements`.

## Indexes (planned minimum)

- `tasks (project_id, key)` UNIQUE
- `tasks (board_id, column_id, order_in_column)`
- `time_logs (user_id, started_at DESC)`
- `task_activity (task_id, created_at DESC)`
- GIN on `tasks.labels`
- GIN/trgm on `tasks.description` and on a maintained `tsvector`

Search uses a `tsvector` column on `tasks` kept current by a trigger, plus
`pg_trgm` for fuzzy matches. Both extensions are enabled in the initial
migration.
