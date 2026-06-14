-- F14: per-client billing. Clients own projects; an invoice rolls up billable
-- time on a client's projects over a period into line items, priced at each
-- contributor's configured hourly rate (cents math, see domain::timesheets).

CREATE TABLE clients (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    email       TEXT,
    address     TEXT,
    currency    TEXT NOT NULL DEFAULT 'USD',
    notes       TEXT NOT NULL DEFAULT '',
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ
);

-- A project belongs to at most one client.
ALTER TABLE projects
    ADD COLUMN client_id UUID REFERENCES clients(id) ON DELETE SET NULL;
CREATE INDEX projects_client_idx ON projects (client_id) WHERE client_id IS NOT NULL;

CREATE TABLE invoices (
    id             UUID PRIMARY KEY,
    client_id      UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    number         TEXT NOT NULL UNIQUE,
    status         TEXT NOT NULL DEFAULT 'draft'
                       CHECK (status IN ('draft', 'sent', 'paid')),
    period_start   DATE NOT NULL,
    period_end     DATE NOT NULL,
    currency       TEXT NOT NULL DEFAULT 'USD',
    subtotal_cents BIGINT NOT NULL DEFAULT 0 CHECK (subtotal_cents >= 0),
    total_cents    BIGINT NOT NULL DEFAULT 0 CHECK (total_cents >= 0),
    notes          TEXT NOT NULL DEFAULT '',
    issued_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at        TIMESTAMPTZ,
    paid_at        TIMESTAMPTZ,
    created_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT invoices_period_order CHECK (period_end >= period_start)
);
CREATE INDEX invoices_client_idx ON invoices (client_id, created_at DESC);

CREATE TABLE invoice_lines (
    id           UUID PRIMARY KEY,
    invoice_id   UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    project_id   UUID REFERENCES projects(id) ON DELETE SET NULL,
    description  TEXT NOT NULL,
    minutes      BIGINT NOT NULL DEFAULT 0,
    rate_cents   BIGINT NOT NULL DEFAULT 0,
    amount_cents BIGINT NOT NULL DEFAULT 0,
    sort         INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX invoice_lines_invoice_idx ON invoice_lines (invoice_id, sort);
