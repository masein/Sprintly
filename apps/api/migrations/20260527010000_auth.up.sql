-- ─────────────────────────────────────────────────────────────────────────────
-- M1 phase 2 — auth tables.
--
-- Tables added here:
--   users                  — accounts. citext email; argon2id hash in
--                            password_hash. role is global ('admin' | 'member'
--                            | 'viewer'). settings JSONB stores theme,
--                            notification prefs, easter-egg toggle.
--   sessions               — a session "family" identified by a stable id.
--                            All refresh tokens for one login chain belong to
--                            one session. Revoking a session revokes every
--                            refresh token in it.
--   refresh_tokens         — one row per refresh token ever issued. token_hash
--                            is SHA-256(secret). On rotation, the old row is
--                            marked rotated_to so we can detect reuse and
--                            revoke the family.
--   invite_tokens          — single-use, admin-generated, surfaced as a copy-
--                            paste link until email sending exists.
--   password_reset_tokens  — single-use, time-bounded reset tokens.
--
-- ID type: UUIDv7 (time-sortable). Generated app-side; column type is uuid.
-- Time columns: TIMESTAMPTZ everywhere. created_at/updated_at default to now().
-- ─────────────────────────────────────────────────────────────────────────────

-- ── users ──────────────────────────────────────────────────────────────────
CREATE TABLE users (
    id              UUID PRIMARY KEY,
    email           CITEXT NOT NULL UNIQUE,
    handle          TEXT   NOT NULL UNIQUE,
    display_name    TEXT   NOT NULL,
    avatar_url      TEXT,
    password_hash   TEXT   NOT NULL,
    role            TEXT   NOT NULL DEFAULT 'member'
                       CHECK (role IN ('admin', 'member', 'viewer')),
    status          TEXT   NOT NULL DEFAULT 'active'
                       CHECK (status IN ('active', 'invited', 'suspended')),
    hourly_rate_cents BIGINT,
    currency        TEXT NOT NULL DEFAULT 'USD',
    timezone        TEXT NOT NULL DEFAULT 'UTC',
    settings        JSONB NOT NULL DEFAULT '{}'::jsonb,
    last_seen_at    TIMESTAMPTZ,
    -- 2FA (off by default; required for admin role at the app layer)
    totp_secret     TEXT,
    totp_enrolled_at TIMESTAMPTZ,
    backup_codes    TEXT[] NOT NULL DEFAULT '{}',

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

-- Soft-delete-aware uniqueness — re-allow an email after deletion.
CREATE UNIQUE INDEX users_email_active_idx ON users (email) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX users_handle_active_idx ON users (handle) WHERE deleted_at IS NULL;

-- ── sessions ───────────────────────────────────────────────────────────────
-- A session represents one login chain. Every refresh-token rotation stays
-- within its session. Revoking a session burns every token in its family.
CREATE TABLE sessions (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent      TEXT,
    ip              INET,
    revoked_at      TIMESTAMPTZ,
    revoked_reason  TEXT,    -- 'logout' | 'reuse_detected' | 'admin' | 'expired'
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_active_idx
    ON sessions (user_id) WHERE revoked_at IS NULL;

-- ── refresh_tokens ─────────────────────────────────────────────────────────
-- token_hash is sha256(secret). The plaintext exists only in the user's
-- cookie. rotated_to / rotated_at let us detect reuse: if we ever see a hash
-- whose rotated_to is non-NULL, the client used a stale token → revoke the
-- session family.
CREATE TABLE refresh_tokens (
    id              UUID PRIMARY KEY,
    session_id      UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      BYTEA NOT NULL UNIQUE,
    issued_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    rotated_to      UUID REFERENCES refresh_tokens(id) ON DELETE SET NULL,
    rotated_at      TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ
);
CREATE INDEX refresh_tokens_session_idx ON refresh_tokens (session_id);
CREATE INDEX refresh_tokens_expires_idx ON refresh_tokens (expires_at);

-- ── invite_tokens ──────────────────────────────────────────────────────────
-- Admin-generated. Single use. token_hash so we never store the secret.
CREATE TABLE invite_tokens (
    id              UUID PRIMARY KEY,
    token_hash      BYTEA NOT NULL UNIQUE,
    email_hint      CITEXT,     -- optional, just to label the row in admin UI
    suggested_role  TEXT NOT NULL DEFAULT 'member'
                       CHECK (suggested_role IN ('admin', 'member', 'viewer')),
    invited_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    consumed_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    consumed_at     TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX invite_tokens_unused_idx
    ON invite_tokens (expires_at) WHERE consumed_at IS NULL;

-- ── password_reset_tokens ──────────────────────────────────────────────────
CREATE TABLE password_reset_tokens (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      BYTEA NOT NULL UNIQUE,
    consumed_at     TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX password_reset_user_idx ON password_reset_tokens (user_id);

-- ── updated_at trigger (boring but useful) ────────────────────────────────
CREATE OR REPLACE FUNCTION sprintly_touch_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_touch_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();
