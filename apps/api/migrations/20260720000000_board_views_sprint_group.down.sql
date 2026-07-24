-- Revert to the original grouping set. Reset any views saved with
-- group_by='sprint' back to 'none' first, otherwise re-adding the narrower
-- constraint would fail on existing rows.
UPDATE board_views SET group_by = 'none' WHERE group_by = 'sprint';
ALTER TABLE board_views DROP CONSTRAINT board_views_group_by_check;
ALTER TABLE board_views ADD CONSTRAINT board_views_group_by_check
    CHECK (group_by IN ('none', 'assignee', 'label', 'priority'));
