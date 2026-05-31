DROP TABLE IF EXISTS task_attachments;
DROP TABLE IF EXISTS task_reactions;
DROP TRIGGER IF EXISTS task_comments_threading_guard ON task_comments;
DROP FUNCTION IF EXISTS sprintly_enforce_one_level_threads();
DROP TABLE IF EXISTS task_comments;
