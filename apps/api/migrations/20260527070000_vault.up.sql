-- ─────────────────────────────────────────────────────────────────────────────
-- M7 — encrypted vault.
--
-- Storage model:
--   vault_items   — one row per secret. `encrypted_payload` is the
--                   XChaCha20-Poly1305 ciphertext, `nonce` is the 24-byte
--                   one-time nonce that was sampled at write time,
--                   `key_version` records which derived key wrapped this row.
--   vault_access  — explicit access grants per (user, item). Project leads
--                   bypass this table at the app layer.
--   vault_audit_log — append-only. The trigger below makes UPDATE/DELETE
--                   raise; the only legitimate way to remove rows is the
--                   admin retention job (M10).
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE vault_items (
    id                 UUID PRIMARY KEY,
    project_id         UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name               TEXT NOT NULL,
    kind               TEXT NOT NULL
                            CHECK (kind IN ('password','api_key','ssh_key','note','env_file')),
    description        TEXT NOT NULL DEFAULT '',

    -- Ciphertext + nonce live in BYTEA. Never appears in API responses
    -- except inside the reveal handler.
    encrypted_payload  BYTEA NOT NULL,
    nonce              BYTEA NOT NULL,
    key_version        INTEGER NOT NULL,

    created_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    last_rotated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at         TIMESTAMPTZ,

    CONSTRAINT vault_items_nonce_length CHECK (octet_length(nonce) = 24)
);
CREATE INDEX vault_items_project_idx
    ON vault_items (project_id, kind, name) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX vault_items_project_name_idx
    ON vault_items (project_id, name) WHERE deleted_at IS NULL;

CREATE TRIGGER vault_items_touch_updated_at
    BEFORE UPDATE ON vault_items
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── vault_access ───────────────────────────────────────────────────────────
CREATE TABLE vault_access (
    vault_item_id      UUID NOT NULL REFERENCES vault_items(id) ON DELETE CASCADE,
    user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    can_view           BOOLEAN NOT NULL DEFAULT true,
    can_edit           BOOLEAN NOT NULL DEFAULT false,
    granted_by         UUID REFERENCES users(id) ON DELETE SET NULL,
    granted_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (vault_item_id, user_id)
);
CREATE INDEX vault_access_user_idx ON vault_access (user_id);

-- ── vault_audit_log (append-only) ──────────────────────────────────────────
CREATE TABLE vault_audit_log (
    id                 UUID PRIMARY KEY,
    vault_item_id      UUID NOT NULL REFERENCES vault_items(id) ON DELETE CASCADE,
    user_id            UUID REFERENCES users(id) ON DELETE SET NULL,
    action             TEXT NOT NULL
                            CHECK (action IN ('created','viewed','revealed','copied','edited','deleted','rotated','access_granted','access_revoked')),
    ip                 INET,
    user_agent         TEXT,
    occurred_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Optional context (e.g. previous_kind on edits). Never carries plaintext.
    payload            JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX vault_audit_log_item_idx
    ON vault_audit_log (vault_item_id, occurred_at DESC);
CREATE INDEX vault_audit_log_user_idx
    ON vault_audit_log (user_id, occurred_at DESC);

-- Defense in depth: nothing about the audit log is mutable. Any UPDATE or
-- DELETE attempt raises and rolls back the transaction. Retention deletes
-- happen via a privileged admin path (M10) that drops + recreates the
-- trigger inside a single transaction.
CREATE OR REPLACE FUNCTION sprintly_block_audit_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'vault_audit_log is append-only';
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER vault_audit_log_no_update
    BEFORE UPDATE ON vault_audit_log
    FOR EACH ROW EXECUTE FUNCTION sprintly_block_audit_mutation();
CREATE TRIGGER vault_audit_log_no_delete
    BEFORE DELETE ON vault_audit_log
    FOR EACH ROW EXECUTE FUNCTION sprintly_block_audit_mutation();
