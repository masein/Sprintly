-- Task ↔ subtask conversion (PUT /tasks/:key/parent) logs a 'reparented'
-- activity row; let the feed's kind CHECK accept it.
ALTER TABLE task_activity DROP CONSTRAINT task_activity_kind_check;
ALTER TABLE task_activity ADD CONSTRAINT task_activity_kind_check CHECK (kind IN (
    'created', 'moved', 'assigned', 'unassigned',
    'estimated', 'titled', 'described', 'commented',
    'time_logged', 'attached', 'linked', 'labeled',
    'prioritized', 'typed', 'completed', 'reopened',
    'watcher_added', 'watcher_removed',
    'commit_linked', 'pr_linked', 'pr_merged',
    'reparented'
));
