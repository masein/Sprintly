-- ─────────────────────────────────────────────────────────────────────────────
-- Sprintly — initial migration. Extensions only. Tables land in M1 phase 2
-- (users, sessions, refresh tokens) and onwards. Keeping this migration tiny
-- and table-free lets us iterate freely until auth ships.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE EXTENSION IF NOT EXISTS "pgcrypto";    -- gen_random_bytes for tokens
CREATE EXTENSION IF NOT EXISTS "pg_trgm";     -- trigram search on tasks
CREATE EXTENSION IF NOT EXISTS "citext";      -- case-insensitive email column
