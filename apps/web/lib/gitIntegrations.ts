// Per-project git provider connections. The webhook secret appears exactly
// once, in the create response — paste it into the provider's webhook form.

import { api } from "./api";

export type GitProvider = "github" | "gitlab" | "gitea";

export type GitIntegration = {
  id: string;
  project_id: string;
  provider: GitProvider;
  repo: string;
  base_url: string | null;
  status_enabled: boolean;
  has_webhook_secret: boolean;
  has_api_token: boolean;
  created_at: string;
};

export const listGitIntegrations = (key: string) =>
  api<GitIntegration[]>(`/projects/${encodeURIComponent(key)}/integrations`);

export const createGitIntegration = (
  key: string,
  body: {
    provider: GitProvider;
    repo: string;
    base_url?: string;
    api_token?: string;
    status_enabled?: boolean;
  },
) =>
  api<{ integration: GitIntegration; webhook_secret: string; webhook_path: string }>(
    `/projects/${encodeURIComponent(key)}/integrations`,
    { method: "POST", body },
  );

export const updateGitIntegration = (
  id: string,
  body: { api_token?: string | null; status_enabled?: boolean },
) => api<GitIntegration>(`/integrations/${id}`, { method: "PATCH", body });

export const deleteGitIntegration = (id: string) =>
  api<void>(`/integrations/${id}`, { method: "DELETE" });
