-- Force-reset flag for provisioned users (Jira import "create missing users").
-- When true, login hands back a force-reset challenge instead of a session;
-- the user must set a new password (which clears the flag) before they get in.

ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN users.must_change_password IS
    'When true, login returns a force-reset challenge instead of a session; cleared once the user sets a new password.';
