-- F7: a per-project label registry giving the free-form task labels a managed
-- palette + colors. Tasks still store labels as a text[] of names; this table
-- maps a name to its colour.
CREATE TABLE project_labels (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        text NOT NULL,
    color       text NOT NULL DEFAULT '#7c5cff',
    created_at  timestamptz NOT NULL DEFAULT now()
);
-- Case-insensitive uniqueness of a label name within a project.
CREATE UNIQUE INDEX project_labels_name_uniq
    ON project_labels (project_id, lower(name));
