-- Best-effort inverse: the up-migration only touched 'todo'-categorised
-- columns named verified/accepted, so sending exactly that set back to
-- 'todo' (and re-opening their tasks) restores the pre-migration state for
-- any column the up actually changed. Columns created AFTER the importer
-- fix (born with category = 'done') match the name predicate too and would
-- be demoted — acceptable for a rollback that accompanies reverting to the
-- older importer, which would have categorised them as 'todo' anyway.

WITH reverted AS (
    UPDATE board_columns
       SET category = 'todo'
     WHERE deleted_at IS NULL
       AND category = 'done'
       AND (lower(name) LIKE '%verified%' OR lower(name) LIKE '%accepted%')
    RETURNING id
)
UPDATE tasks t
   SET status = 'todo',
       completed_at = NULL
  FROM reverted r
 WHERE t.column_id = r.id
   AND t.deleted_at IS NULL;
