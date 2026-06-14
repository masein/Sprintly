DROP INDEX IF EXISTS projects_public_token_idx;
ALTER TABLE projects DROP COLUMN IF EXISTS public_token;
