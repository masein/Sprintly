// Admin panel API surface.

import { api } from "./api";

export type AdminUserRow = {
  id: string;
  email: string;
  handle: string;
  display_name: string;
  role: "admin" | "member" | "viewer";
  status: "active" | "invited" | "suspended";
  created_at: string;
  last_seen_at: string | null;
};

export const listAdminUsers = (params: { q?: string; status?: string; role?: string }) => {
  const qs = new URLSearchParams();
  if (params.q) qs.set("q", params.q);
  if (params.status) qs.set("status", params.status);
  if (params.role) qs.set("role", params.role);
  const suffix = qs.toString();
  return api<{ items: AdminUserRow[] }>(`/admin/users${suffix ? `?${suffix}` : ""}`)
    .then((r) => r.items);
};

export const suspendUser = (id: string) =>
  api<void>(`/admin/users/${id}/suspend`, { method: "POST" });

export const reactivateUser = (id: string) =>
  api<void>(`/admin/users/${id}/reactivate`, { method: "POST" });

export const setUserRole = (id: string, role: AdminUserRow["role"]) =>
  api<void>(`/admin/users/${id}/role`, { method: "POST", body: { role } });

export const resetUserPassword = (id: string) =>
  api<{ token: string; url: string; expires_at: string }>(
    `/admin/users/${id}/reset-password`,
    { method: "POST" },
  );

export const setUserEmail = (id: string, email: string) =>
  api<void>(`/admin/users/${id}/email`, { method: "POST", body: { email } });

// ── invites ─────────────────────────────────────────────────────────────────

export type InviteRow = {
  id: string;
  email_hint: string | null;
  suggested_role: "admin" | "member" | "viewer";
  invited_by: string | null;
  created_at: string;
  expires_at: string;
  consumed_at: string | null;
  consumed_by: string | null;
};

export type CreatedInvite = {
  id: string;
  /** Plaintext token — surfaced once, never returned again. */
  token: string;
  url: string;
  email_hint: string | null;
  suggested_role: string;
  expires_at: string;
};

export const listInvites = () =>
  api<{ items: InviteRow[] }>("/admin/invites").then((r) => r.items);

export const createInvite = (body: {
  email_hint?: string;
  suggested_role?: InviteRow["suggested_role"];
  ttl_hours?: number;
}) => api<CreatedInvite>("/admin/invites", { method: "POST", body });

export const revokeInvite = (id: string) =>
  api<void>(`/admin/invites/${id}/revoke`, { method: "POST" });

export type AdminAuditRow = {
  id: string;
  actor_handle: string | null;
  action: string;
  target_handle: string | null;
  payload: Record<string, unknown>;
  ip: string | null;
  occurred_at: string;
};
export const listAdminAudit = () =>
  api<{ items: AdminAuditRow[] }>("/admin/audit").then((r) => r.items);

export type HealthCheck = { ok: boolean; latency_ms: number; detail: string | null };
export type Health = {
  db: HealthCheck;
  redis: HealthCheck;
  minio: HealthCheck;
  version: string;
  jobs: { pending: number; running: number; failed: number; last_finished_at: string | null };
};
export const getHealth = () => api<Health>("/admin/health");

export type BackupRow = {
  id: string;
  status: "pending" | "running" | "done" | "failed";
  requested_by: string | null;
  started_at: string | null;
  finished_at: string | null;
  size_bytes: number | null;
  storage_key: string | null;
  error: string | null;
  created_at: string;
};
export type BackupPolicy = {
  schedule_secs: number | null;
  retention_count: number | null;
  retention_days: number | null;
};
export const listBackups = () =>
  api<{ items: BackupRow[]; policy: BackupPolicy }>("/admin/backups");
export const startBackup = () =>
  api<BackupRow>("/admin/backups", { method: "POST" });
