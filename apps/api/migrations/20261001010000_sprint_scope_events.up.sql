-- When did each task join or leave each sprint?
--
-- Nothing recorded it: `tasks.sprint_id` is just a pointer, overwritten by
-- assign/unassign, create-with-sprint, carry-over, reparenting and sprint
-- deletion alike. Without the history there is no "scope change" KPI (work
-- added after the sprint started ÷ the scope it started with) and no honest
-- burndown once scope moves mid-sprint (KPI request alongside QA report 6).
--
-- A trigger, rather than code at each call site: every path that changes
-- `sprint_id` is covered, including ones added later, and a missed call site
-- can't quietly corrupt the history.
CREATE TABLE sprint_scope_events (
    id          bigserial   PRIMARY KEY,
    sprint_id   uuid        NOT NULL REFERENCES sprints (id) ON DELETE CASCADE,
    task_id     uuid        NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    change      text        NOT NULL CHECK (change IN ('added', 'removed')),
    at          timestamptz NOT NULL DEFAULT now(),
    -- True for rows reconstructed by this migration rather than observed.
    -- The KPI says "≈" when it leans on them.
    backfilled  boolean     NOT NULL DEFAULT false
);

CREATE INDEX sprint_scope_events_sprint_idx ON sprint_scope_events (sprint_id, at);

CREATE FUNCTION sprintly_track_sprint_scope() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.sprint_id IS NOT NULL THEN
            INSERT INTO sprint_scope_events (sprint_id, task_id, change)
            VALUES (NEW.sprint_id, NEW.id, 'added');
        END IF;
    ELSIF NEW.sprint_id IS DISTINCT FROM OLD.sprint_id THEN
        IF OLD.sprint_id IS NOT NULL THEN
            INSERT INTO sprint_scope_events (sprint_id, task_id, change)
            VALUES (OLD.sprint_id, NEW.id, 'removed');
        END IF;
        IF NEW.sprint_id IS NOT NULL THEN
            INSERT INTO sprint_scope_events (sprint_id, task_id, change)
            VALUES (NEW.sprint_id, NEW.id, 'added');
        END IF;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER tasks_track_sprint_scope
    AFTER INSERT OR UPDATE OF sprint_id ON tasks
    FOR EACH ROW EXECUTE FUNCTION sprintly_track_sprint_scope();

-- Backfill, best effort. A task currently in a sprint joined it no earlier
-- than either existed — so a task created after its sprint started counts as
-- added mid-sprint (true by construction), and one created before counts as
-- part of the original scope (the likeliest story).
INSERT INTO sprint_scope_events (sprint_id, task_id, change, at, backfilled)
SELECT t.sprint_id, t.id, 'added', GREATEST(t.created_at, s.created_at), true
FROM   tasks t
JOIN   sprints s ON s.id = t.sprint_id;

-- Completed sprints remember tasks that have since left (carried over): they
-- joined the same way, and left at completion.
INSERT INTO sprint_scope_events (sprint_id, task_id, change, at, backfilled)
SELECT x.sprint_id, x.task_id, 'added', GREATEST(t.created_at, s.created_at), true
FROM   sprint_task_snapshots x
JOIN   sprints s ON s.id = x.sprint_id
JOIN   tasks t   ON t.id = x.task_id
WHERE  t.sprint_id IS DISTINCT FROM x.sprint_id;

INSERT INTO sprint_scope_events (sprint_id, task_id, change, at, backfilled)
SELECT x.sprint_id, x.task_id, 'removed', x.snapped_at, true
FROM   sprint_task_snapshots x
JOIN   tasks t ON t.id = x.task_id
WHERE  t.sprint_id IS DISTINCT FROM x.sprint_id;
