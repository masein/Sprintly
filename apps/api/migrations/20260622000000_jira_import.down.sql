DROP INDEX IF EXISTS tasks_external_ref_uniq;
ALTER TABLE tasks DROP COLUMN IF EXISTS external_ref;
