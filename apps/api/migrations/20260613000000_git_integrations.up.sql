-- F1: per-project git provider connections (ADR 0001).
--
-- One row per (project, provider, repo). Secrets are vault-encrypted with
-- the per-project key (AAD = integration id): `webhook_secret_*` verifies
-- inbound webhooks, `api_token_*` authenticates outbound API calls
-- (commit status). `base_url` targets self-hosted GitLab/Gitea; NULL means
-- the public cloud instance.

CREATE TABLE git_integrations (
    id                     uuid PRIMARY KEY,
    project_id             uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider               text NOT NULL CHECK (provider IN ('github', 'gitlab', 'gitea')),
    repo                   text NOT NULL,   -- "owner/name" (gitlab: path or numeric id)
    base_url               text,
    webhook_secret_ct      bytea,
    webhook_secret_nonce   bytea,
    api_token_ct           bytea,
    api_token_nonce        bytea,
    -- Push task-state back to the provider as commit statuses.
    status_enabled         boolean NOT NULL DEFAULT false,
    created_by             uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, provider, repo)
);
CREATE INDEX git_integrations_project_idx ON git_integrations (project_id);

-- Outbound status needs the full commit SHA; external_ref only carries the
-- 7-char short form (commits) or "#N" (PRs, where this is the head SHA).
ALTER TABLE git_links ADD COLUMN sha text;
