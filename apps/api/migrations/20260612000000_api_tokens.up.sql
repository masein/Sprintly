-- F12: personal API tokens — scriptable, scoped access to the REST API.
--
-- The secret is `slt_` + base64url(32 random bytes), shown to the user once;
-- we store only sha256 of the raw bytes. Revoke = DELETE (immediate).
-- Scopes: 'read' (GET-only) and 'write' (write implies read).

CREATE TABLE api_tokens (
    id            UUID PRIMARY KEY,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    token_hash    BYTEA NOT NULL UNIQUE,
    scopes        TEXT[] NOT NULL DEFAULT '{read}',
    last_used_at  TIMESTAMPTZ,
    expires_at    TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX api_tokens_user_idx ON api_tokens (user_id);
