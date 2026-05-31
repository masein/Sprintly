DROP TABLE IF EXISTS jobs;
DROP TABLE IF EXISTS task_activity;
DROP TABLE IF EXISTS task_links;
DROP TABLE IF EXISTS task_watchers;
DROP TRIGGER IF EXISTS tasks_search_tsv_trigger ON tasks;
DROP FUNCTION IF EXISTS sprintly_tasks_update_search();
DROP TABLE IF EXISTS tasks;
ALTER TABLE projects DROP COLUMN IF EXISTS next_task_seq;
