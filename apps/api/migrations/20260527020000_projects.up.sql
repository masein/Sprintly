-- ─────────────────────────────────────────────────────────────────────────────
-- M2 — projects, members, boards, columns.
--
-- Cardinality:
--   project 1—* project_members
--   project 1—* boards            (always ≥ 1; default board created with project)
--   board   1—* columns           (default: To do · In progress · Done)
--   project 1—* tasks             (M3)
--
-- Project key (e.g. "WEB") is UNIQUE per instance and immutable. The task
-- ID format "WEB-142" derives from it.
--
-- order_in_column for tasks (M3) is FLOAT8 — fractional ordering lets us drop
-- a card between two others without renumbering. We use the same trick on
-- columns for cheap drag-reorders; rebalance lazily when precision drifts.
-- ─────────────────────────────────────────────────────────────────────────────

-- ── projects ───────────────────────────────────────────────────────────────
CREATE TABLE projects (
    id            UUID PRIMARY KEY,
    key           TEXT NOT NULL,
    name          TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    icon          TEXT NOT NULL DEFAULT 'folder',     -- lucide icon name
    color         TEXT NOT NULL DEFAULT '#7c5cff',    -- hex; UI validates
    archived_at   TIMESTAMPTZ,
    settings      JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by    UUID REFERENCES users(id) ON DELETE SET NULL,

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ,

    -- Format check: 2-10 uppercase letters/digits, starts with letter.
    CONSTRAINT projects_key_format
        CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}$')
);
CREATE UNIQUE INDEX projects_key_active_idx
    ON projects (key) WHERE deleted_at IS NULL;
CREATE INDEX projects_archived_idx
    ON projects (archived_at) WHERE archived_at IS NOT NULL;

CREATE TRIGGER projects_touch_updated_at
    BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── project_members ────────────────────────────────────────────────────────
-- A user's role *within* a project. Layered on top of the global users.role.
-- A global admin can do anything in any project regardless of membership.
CREATE TABLE project_members (
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id       UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    role          TEXT NOT NULL DEFAULT 'contributor'
                     CHECK (role IN ('lead', 'contributor', 'watcher')),
    added_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    added_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (project_id, user_id)
);
CREATE INDEX project_members_user_idx ON project_members (user_id);

-- ── boards ─────────────────────────────────────────────────────────────────
CREATE TABLE boards (
    id            UUID PRIMARY KEY,
    project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name          TEXT NOT NULL DEFAULT 'Board',
    type          TEXT NOT NULL DEFAULT 'kanban'
                     CHECK (type IN ('kanban', 'sprint')),
    is_default    BOOLEAN NOT NULL DEFAULT false,

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ
);
CREATE INDEX boards_project_idx ON boards (project_id) WHERE deleted_at IS NULL;
-- At most one default board per project.
CREATE UNIQUE INDEX boards_default_per_project_idx
    ON boards (project_id) WHERE is_default = true AND deleted_at IS NULL;

CREATE TRIGGER boards_touch_updated_at
    BEFORE UPDATE ON boards
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── columns ────────────────────────────────────────────────────────────────
-- "category" drives analytics (and the dashboard "in progress" totals).
-- "name" is whatever the team wants — "Yeeted to QA" still maps to "review".
CREATE TABLE board_columns (
    id            UUID PRIMARY KEY,
    board_id      UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    category      TEXT NOT NULL
                     CHECK (category IN ('todo', 'in_progress', 'review', 'done')),
    wip_limit     INTEGER,                     -- nullable = no limit
    sort_order    DOUBLE PRECISION NOT NULL,   -- fractional; rebalance on drift

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ,

    CONSTRAINT board_columns_wip_positive CHECK (wip_limit IS NULL OR wip_limit > 0)
);
CREATE INDEX board_columns_board_idx
    ON board_columns (board_id, sort_order) WHERE deleted_at IS NULL;

CREATE TRIGGER board_columns_touch_updated_at
    BEFORE UPDATE ON board_columns
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();
