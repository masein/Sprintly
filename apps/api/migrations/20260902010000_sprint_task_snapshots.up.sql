-- What a sprint looked like the moment it was completed.
--
-- Until now a completed sprint's task list was whatever still had sprint_id
-- pointing at it. Carrying unfinished work into the next sprint therefore
-- rewrote history: the old sprint "lost" the task, its counts changed after
-- the fact, and the time logged during that cycle walked off with the task.
-- QA report 5 asked for the Jira model — the sprint page for a completed
-- sprint shows the tasks in their exact state at completion, with the time
-- logged while the sprint ran.
--
-- One row per (sprint, task), written inside the completion transaction
-- before any carry-over moves anything. Deleting a task later keeps the row:
-- the sprint really did contain it.
CREATE TABLE sprint_task_snapshots (
    sprint_id       uuid        NOT NULL REFERENCES sprints (id) ON DELETE CASCADE,
    task_id         uuid        NOT NULL,
    key             text        NOT NULL,
    title           text        NOT NULL,
    status          text        NOT NULL,
    priority        text        NOT NULL,
    type            text        NOT NULL,
    story_points    integer,
    assignee_id     uuid,
    -- Minutes logged against the task between the sprint starting and
    -- completing. Not "all time ever on this task": that's the task's number.
    logged_minutes  integer     NOT NULL DEFAULT 0,
    snapped_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (sprint_id, task_id)
);
