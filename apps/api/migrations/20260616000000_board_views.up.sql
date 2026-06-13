-- F8: saved board views — a named filter + swimlane grouping a user can save
-- and reopen, optionally sharing it with the rest of the project.
--
-- `filter` is opaque to the backend: the client stores its chip array as JSON
-- and reconstructs both the filter DSL and the chip UI from it on reopen.
-- `group_by` drives client-side swimlane rendering.

CREATE TABLE board_views (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    owner_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        text NOT NULL,
    filter      jsonb NOT NULL DEFAULT '[]'::jsonb,
    group_by    text NOT NULL DEFAULT 'none'
                    CHECK (group_by IN ('none', 'assignee', 'label', 'priority')),
    shared      boolean NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
-- List path: a project's views the caller can see (own + shared).
CREATE INDEX board_views_project_idx ON board_views (project_id);
