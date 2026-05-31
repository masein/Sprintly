-- ─────────────────────────────────────────────────────────────────────────────
-- M8 — payroll.
--
-- Two changes:
--   * projects: optional monthly budget. Stored as cents to match the rest
--     of the money model. NULL = no budget set, no burn-rate widget shown.
--   * payroll_periods: an admin marker per (user, year, month) so that
--     "mark paid" is reversible bookkeeping independent of the weekly
--     timesheet rows.
--
-- Per-period totals are computed on read; we don't snapshot here because the
-- weekly timesheet `total_pay_cents` already snapshots at submit/approve, and
-- summing those is cheap.
-- ─────────────────────────────────────────────────────────────────────────────

ALTER TABLE projects
    ADD COLUMN budget_cents     BIGINT,
    ADD COLUMN budget_currency  TEXT NOT NULL DEFAULT 'USD',
    ADD CONSTRAINT projects_budget_nonneg
        CHECK (budget_cents IS NULL OR budget_cents >= 0);

CREATE TABLE payroll_periods (
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period_year  SMALLINT NOT NULL,
    period_month SMALLINT NOT NULL CHECK (period_month BETWEEN 1 AND 12),
    status       TEXT NOT NULL DEFAULT 'open'
                       CHECK (status IN ('open', 'paid')),
    paid_at      TIMESTAMPTZ,
    paid_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    note         TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (user_id, period_year, period_month)
);
CREATE INDEX payroll_periods_status_idx
    ON payroll_periods (period_year, period_month, status);

CREATE TRIGGER payroll_periods_touch_updated_at
    BEFORE UPDATE ON payroll_periods
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();
