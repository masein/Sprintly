-- ─────────────────────────────────────────────────────────────────────────────
-- M10 — admin & ops.
--
-- Three new tables:
--   * webhooks            — outbound webhook registrations. Delivery itself
--                           is not wired in v1; rows exist so the UI is
--                           usable and the schema stays out of M11's way.
--                           Secret is stored hashed (sha256) so we can verify
--                           an incoming signature later without keeping the
--                           plaintext on disk.
--   * admin_audit_log     — append-only, like vault_audit_log. Anything that
--                           changes a user account or system config writes
--                           a row here.
--   * backups             — bookkeeping for the pg_dump → MinIO job. One
--                           row per attempt, with status + size + storage
--                           key + error.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE webhooks (
    id                UUID PRIMARY KEY,
    project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    url               TEXT NOT NULL,
    secret_hash       BYTEA NOT NULL,
    events            TEXT[] NOT NULL DEFAULT '{}',
    active            BOOLEAN NOT NULL DEFAULT true,
    last_delivery_at  TIMESTAMPTZ,
    last_status       INTEGER,
    created_by        UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ
);
CREATE INDEX webhooks_project_idx
    ON webhooks (project_id) WHERE deleted_at IS NULL;
CREATE TRIGGER webhooks_touch_updated_at
    BEFORE UPDATE ON webhooks
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

CREATE TABLE admin_audit_log (
    id              UUID PRIMARY KEY,
    actor_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    action          TEXT NOT NULL,        -- e.g. 'user.suspend', 'user.role', 'backup.start'
    target_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip              INET,
    user_agent      TEXT,
    occurred_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX admin_audit_log_occurred_idx
    ON admin_audit_log (occurred_at DESC);
CREATE INDEX admin_audit_log_actor_idx
    ON admin_audit_log (actor_id, occurred_at DESC);

-- Reuse the same append-only trigger function from M7.
CREATE TRIGGER admin_audit_log_no_update
    BEFORE UPDATE ON admin_audit_log
    FOR EACH ROW EXECUTE FUNCTION sprintly_block_audit_mutation();
CREATE TRIGGER admin_audit_log_no_delete
    BEFORE DELETE ON admin_audit_log
    FOR EACH ROW EXECUTE FUNCTION sprintly_block_audit_mutation();

CREATE TABLE backups (
    id              UUID PRIMARY KEY,
    requested_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                         CHECK (status IN ('pending', 'running', 'done', 'failed')),
    started_at      TIMESTAMPTZ,
    finished_at     TIMESTAMPTZ,
    size_bytes      BIGINT,
    storage_key     TEXT,
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX backups_status_created_idx
    ON backups (status, created_at DESC);
