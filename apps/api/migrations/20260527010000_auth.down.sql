DROP TRIGGER IF EXISTS users_touch_updated_at ON users;
DROP FUNCTION IF EXISTS sprintly_touch_updated_at();
DROP TABLE IF EXISTS password_reset_tokens;
DROP TABLE IF EXISTS invite_tokens;
DROP TABLE IF EXISTS refresh_tokens;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS users;
