-- ─────────────────────────────────────────────────────────────────────────────
-- M4 — time tracking.
--
-- Invariants worth highlighting:
--
--   * `time_logs.ended_at IS NULL`  → this log is RUNNING.
--   * One running log per user, enforced by a partial unique index.
--   * `duration_minutes` is generated from started_at / ended_at when the log
--     is closed. Stored, not computed at read time, so listing endpoints
--     never re-do math.
--
-- Timesheets are per-week (Mon-Sun). We don't pre-compute timesheet rows on
-- log writes — the row exists once for submission/approval bookkeeping. The
-- totals reflect a snapshot taken at submit/approve time so an approved
-- timesheet is immutable even if logs change later (they shouldn't, but
-- defense in depth).
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE time_logs (
    id                 UUID PRIMARY KEY,
    task_id            UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    started_at         TIMESTAMPTZ NOT NULL,
    ended_at           TIMESTAMPTZ,
    -- Generated stored column so we never compute on read.
    duration_minutes   INTEGER GENERATED ALWAYS AS (
        CASE
            WHEN ended_at IS NULL THEN NULL
            ELSE GREATEST(0, FLOOR(EXTRACT(EPOCH FROM (ended_at - started_at)) / 60))::INTEGER
        END
    ) STORED,

    note               TEXT NOT NULL DEFAULT '',
    billable           BOOLEAN NOT NULL DEFAULT true,

    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at         TIMESTAMPTZ,

    -- Closed entries must have a sane interval.
    CONSTRAINT time_logs_endorder
        CHECK (ended_at IS NULL OR ended_at >= started_at)
);

-- Hot path: my logs this week, by recency.
CREATE INDEX time_logs_user_started_idx
    ON time_logs (user_id, started_at DESC)
    WHERE deleted_at IS NULL;
-- Hot path: task page "logs on this task".
CREATE INDEX time_logs_task_idx
    ON time_logs (task_id) WHERE deleted_at IS NULL;
-- Range scan when computing a week's logs by user.
CREATE INDEX time_logs_user_range_idx
    ON time_logs (user_id, started_at)
    WHERE deleted_at IS NULL AND ended_at IS NOT NULL;

-- One running log per user. The partial uniqueness is the entire invariant.
CREATE UNIQUE INDEX time_logs_one_running_per_user
    ON time_logs (user_id) WHERE ended_at IS NULL AND deleted_at IS NULL;

CREATE TRIGGER time_logs_touch_updated_at
    BEFORE UPDATE ON time_logs
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── timesheets ─────────────────────────────────────────────────────────────
CREATE TABLE timesheets (
    id                 UUID PRIMARY KEY,
    user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period_start       DATE NOT NULL,     -- Monday (UTC)
    period_end         DATE NOT NULL,     -- Sunday (UTC)
    status             TEXT NOT NULL DEFAULT 'open'
                            CHECK (status IN ('open', 'submitted', 'approved', 'paid')),
    approver_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    submitted_at       TIMESTAMPTZ,
    approved_at        TIMESTAMPTZ,
    paid_at            TIMESTAMPTZ,

    -- Snapshot at submit/approve.
    total_minutes      INTEGER NOT NULL DEFAULT 0,
    billable_minutes   INTEGER NOT NULL DEFAULT 0,
    total_pay_cents    BIGINT NOT NULL DEFAULT 0,
    currency           TEXT NOT NULL DEFAULT 'USD',

    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT timesheets_period_order CHECK (period_end >= period_start)
);
CREATE UNIQUE INDEX timesheets_user_period_idx
    ON timesheets (user_id, period_start);

CREATE TRIGGER timesheets_touch_updated_at
    BEFORE UPDATE ON timesheets
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();
