-- Subtasks get a hand-set order (QA report 6: drag-and-drop subtask
-- reordering). NULL means "never arranged": those sort after arranged ones, by
-- creation time — so a new subtask lands at the end of a custom order without
-- anyone having to compute a position for it.
ALTER TABLE tasks ADD COLUMN subtask_position INTEGER;

-- Start every existing family in its current (creation) order, so the first
-- drag on an old task reorders what people already see.
UPDATE tasks t
SET    subtask_position = o.n
FROM  (
    SELECT id, row_number() OVER (PARTITION BY parent_task_id ORDER BY created_at, id) AS n
    FROM   tasks
    WHERE  parent_task_id IS NOT NULL AND deleted_at IS NULL
) o
WHERE  t.id = o.id;

CREATE INDEX tasks_subtask_order_idx
    ON tasks (parent_task_id, subtask_position)
    WHERE parent_task_id IS NOT NULL AND deleted_at IS NULL;
