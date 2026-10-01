-- Setting or clearing a custom field value logs a 'field_set' / 'field_cleared'
-- activity row, but the feed's kind CHECK never listed them — so every save of
-- a custom field value failed the insert and answered 500, for everyone.
-- Widen the list (same shape as the 'reparented' widening before it).
ALTER TABLE task_activity DROP CONSTRAINT task_activity_kind_check;
ALTER TABLE task_activity ADD CONSTRAINT task_activity_kind_check CHECK (kind IN (
    'created', 'moved', 'assigned', 'unassigned',
    'estimated', 'titled', 'described', 'commented',
    'time_logged', 'attached', 'linked', 'labeled',
    'prioritized', 'typed', 'completed', 'reopened',
    'watcher_added', 'watcher_removed',
    'commit_linked', 'pr_linked', 'pr_merged',
    'reparented',
    'field_set', 'field_cleared'
));
