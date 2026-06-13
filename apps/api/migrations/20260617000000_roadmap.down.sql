-- Reverse order: drop the FK column before the table it references.
DROP INDEX IF EXISTS tasks_epic_idx;
ALTER TABLE tasks DROP COLUMN IF EXISTS epic_id;
DROP TABLE IF EXISTS milestones;
DROP TABLE IF EXISTS epics;
