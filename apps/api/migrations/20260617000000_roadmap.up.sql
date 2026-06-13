-- F6: roadmap / timeline — epics (date-ranged, coloured groupings of tasks)
-- and milestones (a dated target). Tasks optionally belong to one epic; epic
-- progress is done/total of its tasks, computed at read time.

CREATE TABLE epics (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        text NOT NULL,
    color       text NOT NULL DEFAULT '#7c5cff',
    -- Nullable: an epic can exist before it's scheduled; the timeline only
    -- draws a bar once it has a start and end.
    start_date  date,
    end_date    date,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX epics_project_idx ON epics (project_id);

CREATE TABLE milestones (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        text NOT NULL,        -- the target, e.g. "Beta cut"
    due_date    date NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX milestones_project_idx ON milestones (project_id);

-- A task belongs to at most one epic; dropping the epic unassigns its tasks.
ALTER TABLE tasks ADD COLUMN epic_id uuid REFERENCES epics(id) ON DELETE SET NULL;
CREATE INDEX tasks_epic_idx ON tasks (epic_id)
    WHERE epic_id IS NOT NULL AND deleted_at IS NULL;
