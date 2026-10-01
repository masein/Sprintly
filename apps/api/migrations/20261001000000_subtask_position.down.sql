DROP INDEX IF EXISTS tasks_subtask_order_idx;
ALTER TABLE tasks DROP COLUMN IF EXISTS subtask_position;
