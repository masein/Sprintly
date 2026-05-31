// Vault API surface.
//
// Critical UI invariants enforced by callers:
//   • Revealed plaintext is never written into TanStack cache, Zustand,
//     localStorage, or any other persistent store.
//   • Revealed plaintext is held in component-local React state and
//     wiped on unmount and on a timer (10s default).
//   • Clipboard content is overwritten (or cleared) 30s after copy.

import { api } from "./api";

export type VaultKind = "password" | "api_key" | "ssh_key" | "note" | "env_file";

export type VaultItem = {
  id: string;
  project_id: string;
  project_key: string;
  name: string;
  kind: VaultKind;
  description: string;
  key_version: number;
  created_by: string | null;
  last_rotated_at: string;
  created_at: string;
  updated_at: string;
};

export type VaultAccessRow = {
  user_id: string;
  handle: string;
  display_name: string;
  can_view: boolean;
  can_edit: boolean;
  granted_at: string;
};

export type VaultAuditRow = {
  id: string;
  user_handle: string | null;
  action: string;
  ip: string | null;
  user_agent: string | null;
  occurred_at: string;
};

export const listVaultItems = (projectKey: string) =>
  api<{ items: VaultItem[] }>(
    `/projects/${encodeURIComponent(projectKey)}/vault`,
  ).then((r) => r.items);

export const createVaultItem = (
  projectKey: string,
  body: { name: string; kind: VaultKind; description?: string; value: string },
) =>
  api<VaultItem>(`/projects/${encodeURIComponent(projectKey)}/vault`, {
    method: "POST",
    body,
  });

export const editVaultItem = (
  id: string,
  body: { name?: string; description?: string; value?: string },
) => api<VaultItem>(`/vault/${id}`, { method: "PATCH", body });

export const deleteVaultItem = (id: string) =>
  api<void>(`/vault/${id}`, { method: "DELETE" });

/** Server returns plaintext exactly once. Never persist. */
export const revealVaultItem = (id: string) =>
  api<{ id: string; value: string }>(`/vault/${id}/reveal`, { method: "POST" });

export const markCopied = (id: string) =>
  api<void>(`/vault/${id}/copied`, { method: "POST" });

export const listVaultAccess = (id: string) =>
  api<{ items: VaultAccessRow[] }>(`/vault/${id}/access`).then((r) => r.items);

export const grantVaultAccess = (
  id: string,
  body: { user_id: string; can_view?: boolean; can_edit?: boolean },
) => api<void>(`/vault/${id}/access`, { method: "POST", body });

export const revokeVaultAccess = (id: string, userId: string) =>
  api<void>(
    `/vault/${id}/access/${encodeURIComponent(userId)}`,
    { method: "DELETE" },
  );

export const listVaultAudit = (id: string) =>
  api<{ items: VaultAuditRow[] }>(`/vault/${id}/audit`).then((r) => r.items);
