-- F18: opt-in public read-only status pages. A project with a non-null
-- public_token exposes a whitelisted summary at /public/status/<token>.
-- Off by default; rotating/clearing the token invalidates the URL.

ALTER TABLE projects ADD COLUMN public_token TEXT;
CREATE UNIQUE INDEX projects_public_token_idx
    ON projects (public_token)
    WHERE public_token IS NOT NULL;
