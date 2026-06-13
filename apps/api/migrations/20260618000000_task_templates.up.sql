-- F9: task templates (project-scoped task skeletons) with an optional
-- recurrence the background worker materialises into real tasks.
--
-- `next_run_at` is the next time a recurring template spawns a task; the
-- worker scans for due rows, creates the task, and advances it. NULL when the
-- template is one-shot (`recurrence = 'none'`) or not yet scheduled.

CREATE TABLE task_templates (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        text NOT NULL,                    -- the template's own name
    title       text NOT NULL,                    -- prefilled task title
    description text NOT NULL DEFAULT '',
    type        text NOT NULL DEFAULT 'feature'
                    CHECK (type IN ('feature', 'bug', 'chore', 'spike', 'incident')),
    priority    text NOT NULL DEFAULT 'p2'
                    CHECK (priority IN ('p0', 'p1', 'p2', 'p3')),
    labels      text[] NOT NULL DEFAULT '{}',
    recurrence  text NOT NULL DEFAULT 'none'
                    CHECK (recurrence IN ('none', 'daily', 'weekly', 'monthly')),
    next_run_at timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX task_templates_project_idx ON task_templates (project_id);
-- Hot path for the worker: due recurring templates.
CREATE INDEX task_templates_due_idx ON task_templates (next_run_at)
    WHERE recurrence <> 'none' AND next_run_at IS NOT NULL;
