-- QA report 5: Jira-imported tasks incorrectly flagged as overdue.
--
-- Root cause: the import category inference knew done/complete/closed/shipped/
-- verified/accepted, but not the rest of Jira's terminal vocabulary — Resolved
-- (the default workflow's main end state), Released, Won't Do, Cancelled,
-- Rejected, Duplicate, Obsolete, Invalid, Fixed, Finished, Archived, Deployed,
-- Live. Those columns fell through to the 'todo' fallback, their tasks stayed
-- status='todo', and every overdue query (status <> 'done' AND due_date < today)
-- listed them — some hundreds of days "overdue" for work Jira had closed.
--
-- The importer is fixed in the same release; this repairs rows imported before
-- it. Same narrow scope as the Verified repair: only 'todo'-categorised columns
-- whose NAME carries one of these terms. Columns someone explicitly set to
-- review/in_progress are left alone.

WITH fixed AS (
    UPDATE board_columns
       SET category = 'done'
     WHERE deleted_at IS NULL
       AND category = 'todo'
       AND (   lower(name) LIKE '%resolved%'
            OR lower(name) LIKE '%released%'
            OR lower(name) LIKE '%deployed%'
            OR lower(name) LIKE '%fixed%'
            OR lower(name) LIKE '%finished%'
            OR lower(name) LIKE '%archived%'
            OR lower(name) LIKE '%won''t%'
            OR lower(name) LIKE '%wont%'
            OR lower(name) LIKE '%cancel%'
            OR lower(name) LIKE '%reject%'
            OR lower(name) LIKE '%duplicate%'
            OR lower(name) LIKE '%obsolete%'
            OR lower(name) LIKE '%invalid%'
            OR lower(name) = 'live')
    RETURNING id
)
UPDATE tasks t
   SET status = 'done',
       completed_at = COALESCE(t.completed_at, now())
  FROM fixed f
 WHERE t.column_id = f.id
   AND t.deleted_at IS NULL
   AND t.status <> 'done';

-- Imported tasks that were already 'done' but never got a completion stamp
-- (the importer didn't set one). Velocity and burndown read completed_at;
-- give them the best date we have.
UPDATE tasks
   SET completed_at = COALESCE(completed_at, updated_at)
 WHERE deleted_at IS NULL
   AND status = 'done'
   AND completed_at IS NULL
   AND external_ref IS NOT NULL;
