-- ─────────────────────────────────────────────────────────────────────────────
-- M5 — sprints + retros.
--
-- One default board per project (M2) coexists with multiple sprints. A task's
-- sprint_id is nullable — tasks live on the board regardless. Closing a
-- sprint just freezes its `velocity_points` and opens a retro.
--
-- Retros are a 1-1 with sprints (one retro per sprint), enforced by
-- sprint_id UNIQUE on sprint_retros.
--
-- Retro notes can be anonymous: `anonymous = true` means the UI should hide
-- author_id even if it's populated. We keep author_id on the row for moderation
-- (admin can still see who wrote it from the audit log later). For most
-- "anonymous" submissions we just store NULL — defense in depth against a
-- careless future endpoint.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE sprints (
    id              UUID PRIMARY KEY,
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    goal            TEXT NOT NULL DEFAULT '',
    starts_at       TIMESTAMPTZ NOT NULL,
    ends_at         TIMESTAMPTZ NOT NULL,
    state           TEXT NOT NULL DEFAULT 'planned'
                         CHECK (state IN ('planned', 'active', 'completed')),
    velocity_points INTEGER,        -- snapshot at completion
    summary_md      TEXT,           -- written when the retro is closed

    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,

    CONSTRAINT sprints_dates_order CHECK (ends_at > starts_at)
);
CREATE INDEX sprints_project_idx
    ON sprints (project_id, starts_at DESC) WHERE deleted_at IS NULL;
-- At most one ACTIVE sprint per project. (The team can have many planned.)
CREATE UNIQUE INDEX sprints_one_active_per_project_idx
    ON sprints (project_id) WHERE state = 'active' AND deleted_at IS NULL;

CREATE TRIGGER sprints_touch_updated_at
    BEFORE UPDATE ON sprints
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── now we can wire the deferred FK on tasks.sprint_id ─────────────────────
ALTER TABLE tasks
    ADD CONSTRAINT tasks_sprint_id_fkey
    FOREIGN KEY (sprint_id) REFERENCES sprints(id) ON DELETE SET NULL;
CREATE INDEX tasks_sprint_idx
    ON tasks (sprint_id) WHERE sprint_id IS NOT NULL AND deleted_at IS NULL;

-- ── sprint_retros ──────────────────────────────────────────────────────────
CREATE TABLE sprint_retros (
    id              UUID PRIMARY KEY,
    sprint_id       UUID NOT NULL UNIQUE REFERENCES sprints(id) ON DELETE CASCADE,
    state           TEXT NOT NULL DEFAULT 'open'
                         CHECK (state IN ('open', 'closed')),
    closed_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER sprint_retros_touch_updated_at
    BEFORE UPDATE ON sprint_retros
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── retro_notes ────────────────────────────────────────────────────────────
CREATE TABLE retro_notes (
    id              UUID PRIMARY KEY,
    retro_id        UUID NOT NULL REFERENCES sprint_retros(id) ON DELETE CASCADE,
    author_id       UUID REFERENCES users(id) ON DELETE SET NULL,
    column_kind     TEXT NOT NULL
                         CHECK (column_kind IN ('went_well', 'went_poorly', 'action_item', 'kudos')),
    body            TEXT NOT NULL,
    anonymous       BOOLEAN NOT NULL DEFAULT false,
    sort_order      DOUBLE PRECISION NOT NULL DEFAULT 1024.0,
    promoted_task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);
CREATE INDEX retro_notes_retro_idx
    ON retro_notes (retro_id, column_kind, sort_order) WHERE deleted_at IS NULL;

CREATE TRIGGER retro_notes_touch_updated_at
    BEFORE UPDATE ON retro_notes
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── retro_votes ────────────────────────────────────────────────────────────
CREATE TABLE retro_votes (
    retro_note_id   UUID NOT NULL REFERENCES retro_notes(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (retro_note_id, user_id)
);
CREATE INDEX retro_votes_user_idx ON retro_votes (user_id);
