-- Per-user email delivery preferences.
--
-- Deliberately its own table rather than a key in `users.settings`: settings is
-- client-writable through PATCH /users/me, and `unsubscribe_token` is a secret
-- the client must never be able to read or set — anyone holding it can turn a
-- person's mail off.
--
-- `mode` is *how* you get mail; `kinds` is *what* is eligible. Comments are
-- off by default because they're the chatty kind: a busy task would otherwise
-- mail everyone watching it all day. Mentions and assignments are things aimed
-- at a person, so they're on.
CREATE TABLE email_prefs (
    user_id           uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    mode              text        NOT NULL DEFAULT 'immediate',
    kinds             jsonb       NOT NULL DEFAULT '{"mention": true, "assigned": true, "comment": false}'::jsonb,
    -- Local hour (0–23) the digest is sent, in the user's own timezone.
    digest_hour       smallint    NOT NULL DEFAULT 8,
    -- 32 random bytes; the one-click unsubscribe link's only credential.
    unsubscribe_token bytea       NOT NULL,
    last_digest_at    timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT email_prefs_mode_check  CHECK (mode IN ('off', 'immediate', 'digest')),
    CONSTRAINT email_prefs_hour_check  CHECK (digest_hour BETWEEN 0 AND 23),
    CONSTRAINT email_prefs_token_len   CHECK (octet_length(unsubscribe_token) = 32)
);

-- The unsubscribe link looks a row up by token, so it must be unique and fast.
CREATE UNIQUE INDEX email_prefs_unsubscribe_idx ON email_prefs (unsubscribe_token);

-- The digest worker asks "who wants a digest?" every few minutes.
CREATE INDEX email_prefs_digest_idx ON email_prefs (digest_hour) WHERE mode = 'digest';

-- Every existing account gets a row with the defaults, so the unsubscribe link
-- in the first email we ever send them already resolves.
INSERT INTO email_prefs (user_id, unsubscribe_token)
SELECT id, gen_random_bytes(32) FROM users
ON CONFLICT (user_id) DO NOTHING;
