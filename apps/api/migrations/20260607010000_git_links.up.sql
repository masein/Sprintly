-- F1: links from Git commits / pull requests / branches to tasks, populated
-- from inbound GitHub webhooks that reference a task key (e.g. "DEMO-1").
CREATE TABLE git_links (
    id            uuid PRIMARY KEY,
    task_id       uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    provider      text NOT NULL DEFAULT 'github',
    kind          text NOT NULL CHECK (kind IN ('commit', 'pull_request', 'branch')),
    external_ref  text NOT NULL,          -- short SHA / PR number / branch name
    url           text,
    title         text,
    state         text,                   -- PRs: open | merged | closed
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (task_id, provider, kind, external_ref)
);
CREATE INDEX git_links_task_idx ON git_links (task_id, created_at DESC);

-- Allow the new git-driven activity kinds in the activity feed.
ALTER TABLE task_activity DROP CONSTRAINT task_activity_kind_check;
ALTER TABLE task_activity ADD CONSTRAINT task_activity_kind_check CHECK (kind IN (
    'created', 'moved', 'assigned', 'unassigned',
    'estimated', 'titled', 'described', 'commented',
    'time_logged', 'attached', 'linked', 'labeled',
    'prioritized', 'typed', 'completed', 'reopened',
    'watcher_added', 'watcher_removed',
    'commit_linked', 'pr_linked', 'pr_merged'
));
