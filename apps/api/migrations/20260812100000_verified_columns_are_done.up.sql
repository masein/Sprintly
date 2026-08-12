-- QA report 4: tasks sitting in a "Verified" column showed up as overdue.
--
-- Root cause: the import category inference matched "verify" (the review
-- gate) but not the past-tense "verified" (work that passed it), so imported
-- Verified columns fell through to the 'todo' fallback. Every overdue query
-- filters on status <> 'done', and status mirrors the column's category —
-- so those tasks stayed "open" forever, some 500+ days overdue.
--
-- The importer is fixed in the same release; this repairs rows that were
-- imported before the fix. Scope is deliberately narrow: only columns whose
-- NAME says verified/accepted and whose category landed in the 'todo'
-- fallback. Columns someone explicitly set to review/in_progress are left
-- alone.

WITH fixed AS (
    UPDATE board_columns
       SET category = 'done'
     WHERE deleted_at IS NULL
       AND category = 'todo'
       AND (lower(name) LIKE '%verified%' OR lower(name) LIKE '%accepted%')
    RETURNING id
)
UPDATE tasks t
   SET status = 'done',
       completed_at = COALESCE(t.completed_at, now())
  FROM fixed f
 WHERE t.column_id = f.id
   AND t.deleted_at IS NULL
   AND t.status <> 'done';
