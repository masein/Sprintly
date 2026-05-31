DROP TABLE IF EXISTS payroll_periods;
ALTER TABLE projects DROP CONSTRAINT IF EXISTS projects_budget_nonneg;
ALTER TABLE projects
    DROP COLUMN IF EXISTS budget_cents,
    DROP COLUMN IF EXISTS budget_currency;
