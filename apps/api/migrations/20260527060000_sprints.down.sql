DROP TABLE IF EXISTS retro_votes;
DROP TABLE IF EXISTS retro_notes;
DROP TABLE IF EXISTS sprint_retros;
ALTER TABLE tasks DROP CONSTRAINT IF EXISTS tasks_sprint_id_fkey;
DROP INDEX IF EXISTS tasks_sprint_idx;
DROP TABLE IF EXISTS sprints;
