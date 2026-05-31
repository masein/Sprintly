-- ─────────────────────────────────────────────────────────────────────────────
-- M3 phase A — tasks + supporting tables.
--
-- Per-project monotonic key: instead of one sequence shared across all
-- projects (would mean WEB-1, MOB-2, WEB-3) we want WEB-1, WEB-2, … and
-- MOB-1, MOB-2, …. We store the next free integer on the project row
-- (`next_task_seq`) and increment it under a transactional update inside
-- task INSERTs. That's atomic without external advisory locks.
--
-- order_in_column is FLOAT8 so reorders just compute (prev + next) / 2.
-- We rebalance during the move endpoint when precision narrows.
-- ─────────────────────────────────────────────────────────────────────────────

-- Per-project key counter. Atomic via UPDATE … RETURNING.
ALTER TABLE projects
    ADD COLUMN next_task_seq BIGINT NOT NULL DEFAULT 1;

-- ── tasks ──────────────────────────────────────────────────────────────────
CREATE TABLE tasks (
    id                UUID PRIMARY KEY,
    project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    board_id          UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    column_id         UUID NOT NULL REFERENCES board_columns(id) ON DELETE RESTRICT,
    key               TEXT NOT NULL,
    title             TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',

    type              TEXT NOT NULL DEFAULT 'feature'
                           CHECK (type IN ('feature', 'bug', 'chore', 'spike', 'incident')),
    priority          TEXT NOT NULL DEFAULT 'p2'
                           CHECK (priority IN ('p0', 'p1', 'p2', 'p3')),
    status            TEXT NOT NULL DEFAULT 'todo'
                           CHECK (status IN ('todo', 'in_progress', 'review', 'done')),

    assignee_id       UUID REFERENCES users(id) ON DELETE SET NULL,
    reporter_id       UUID REFERENCES users(id) ON DELETE SET NULL,
    parent_task_id    UUID REFERENCES tasks(id) ON DELETE SET NULL,
    sprint_id         UUID,    -- FK added in M5

    estimate_minutes  INTEGER,
    story_points      INTEGER,
    due_date          DATE,
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,

    labels            TEXT[]  NOT NULL DEFAULT '{}',
    custom_fields     JSONB   NOT NULL DEFAULT '{}'::jsonb,

    order_in_column   DOUBLE PRECISION NOT NULL,

    -- Full-text search column; maintained by a trigger below.
    search_tsv        TSVECTOR,

    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ
);

-- Per-project unique task key. Composite unique, soft-delete aware.
CREATE UNIQUE INDEX tasks_project_key_idx
    ON tasks (project_id, key) WHERE deleted_at IS NULL;
-- Hot path: board scan.
CREATE INDEX tasks_board_column_order_idx
    ON tasks (board_id, column_id, order_in_column) WHERE deleted_at IS NULL;
-- Personal filters.
CREATE INDEX tasks_assignee_idx
    ON tasks (assignee_id) WHERE deleted_at IS NULL AND assignee_id IS NOT NULL;
-- Label search.
CREATE INDEX tasks_labels_gin_idx ON tasks USING GIN (labels);
-- Trigram + tsvector search.
CREATE INDEX tasks_title_trgm_idx ON tasks USING GIN (title gin_trgm_ops);
CREATE INDEX tasks_search_tsv_idx ON tasks USING GIN (search_tsv);

-- tsvector trigger — recomputes on title/description/labels changes.
CREATE OR REPLACE FUNCTION sprintly_tasks_update_search() RETURNS trigger AS $$
BEGIN
    NEW.search_tsv :=
        setweight(to_tsvector('simple', coalesce(NEW.key, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'B') ||
        setweight(to_tsvector('simple', coalesce(array_to_string(NEW.labels, ' '), '')), 'C');
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER tasks_search_tsv_trigger
    BEFORE INSERT OR UPDATE OF title, description, labels, key ON tasks
    FOR EACH ROW EXECUTE FUNCTION sprintly_tasks_update_search();

CREATE TRIGGER tasks_touch_updated_at
    BEFORE UPDATE ON tasks
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── task_watchers ──────────────────────────────────────────────────────────
CREATE TABLE task_watchers (
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    added_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, user_id)
);
CREATE INDEX task_watchers_user_idx ON task_watchers (user_id);

-- ── task_links ─────────────────────────────────────────────────────────────
-- Directed edges. (blocks, A→B) ⇒ "A blocks B". UI inverts as needed.
CREATE TABLE task_links (
    from_task_id  UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    to_task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL
                       CHECK (kind IN ('blocks', 'relates_to', 'duplicates', 'parent_of')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (from_task_id, to_task_id, kind),
    CHECK (from_task_id <> to_task_id)
);
CREATE INDEX task_links_to_idx ON task_links (to_task_id, kind);

-- ── task_activity ──────────────────────────────────────────────────────────
-- Append-only activity feed. Source of truth for /tasks/:key history.
CREATE TABLE task_activity (
    id            UUID PRIMARY KEY,
    task_id       UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    actor_id      UUID REFERENCES users(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL
                       CHECK (kind IN (
                           'created', 'moved', 'assigned', 'unassigned',
                           'estimated', 'titled', 'described', 'commented',
                           'time_logged', 'attached', 'linked', 'labeled',
                           'prioritized', 'typed', 'completed', 'reopened',
                           'watcher_added', 'watcher_removed'
                       )),
    payload       JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX task_activity_task_created_idx
    ON task_activity (task_id, created_at DESC);

-- ── jobs ───────────────────────────────────────────────────────────────────
-- Durable background work. The in-process worker (Tokio) picks rows up,
-- marks them running with `claimed_at`, finishes or re-queues with exponential
-- backoff. Schema lives here even though the runner lands later — keeps the
-- migration history clean.
CREATE TABLE jobs (
    id            UUID PRIMARY KEY,
    kind          TEXT NOT NULL,
    payload       JSONB NOT NULL DEFAULT '{}'::jsonb,
    run_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts      INTEGER NOT NULL DEFAULT 0,
    max_attempts  INTEGER NOT NULL DEFAULT 10,
    claimed_at    TIMESTAMPTZ,
    finished_at   TIMESTAMPTZ,
    last_error    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX jobs_runnable_idx
    ON jobs (run_at)
    WHERE finished_at IS NULL AND claimed_at IS NULL;
