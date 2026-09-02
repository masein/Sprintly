-- Best-effort inverse, mirroring the Verified repair's down: send the columns
-- this migration could have promoted back to 'todo' and reopen their tasks.
-- Columns created after the importer fix (born 'done') match the name
-- predicate too and would be demoted — acceptable for a rollback that
-- accompanies reverting to the older importer.

WITH reverted AS (
    UPDATE board_columns
       SET category = 'todo'
     WHERE deleted_at IS NULL
       AND category = 'done'
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
   SET status = 'todo',
       completed_at = NULL
  FROM reverted r
 WHERE t.column_id = r.id
   AND t.deleted_at IS NULL;
-- The completed_at backfill for already-done imported tasks is not reversed:
-- there is no record of which rows were NULL before, and a stamp on a done
-- task is harmless.
