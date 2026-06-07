DROP TABLE IF EXISTS git_links;

-- Restore the original activity-kind constraint.
ALTER TABLE task_activity DROP CONSTRAINT task_activity_kind_check;
ALTER TABLE task_activity ADD CONSTRAINT task_activity_kind_check CHECK (kind IN (
    'created', 'moved', 'assigned', 'unassigned',
    'estimated', 'titled', 'described', 'commented',
    'time_logged', 'attached', 'linked', 'labeled',
    'prioritized', 'typed', 'completed', 'reopened',
    'watcher_added', 'watcher_removed'
));
