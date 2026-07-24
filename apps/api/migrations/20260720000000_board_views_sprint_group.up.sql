-- Allow 'sprint' as a board-view swimlane grouping, so a view grouped by
-- sprint (active sprint / other sprints / backlog lanes) can be saved and
-- reopened like any other grouping. Widens the CHECK added in
-- 20260616000000_board_views.up.sql (Postgres auto-named the inline column
-- constraint `board_views_group_by_check`).
ALTER TABLE board_views DROP CONSTRAINT board_views_group_by_check;
ALTER TABLE board_views ADD CONSTRAINT board_views_group_by_check
    CHECK (group_by IN ('none', 'assignee', 'label', 'priority', 'sprint'));
