-- TOTP replay protection (RFC 6238 §5.2): the time step of the last accepted
-- code. A code is only accepted for a step strictly greater than this, so a
-- code that was already used — or an older one still inside the ±1-step skew
-- window — can't be replayed. Reset when enrollment starts over or 2FA is off.

ALTER TABLE users ADD COLUMN totp_last_step BIGINT;

COMMENT ON COLUMN users.totp_last_step IS
    'TOTP time step (unix_secs / 30) of the last accepted code; codes for this step or earlier are refused.';
