// Public read-only status pages (F18).

import { api } from "./api";

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

export type PublicSprint = {
  name: string;
  starts_at: string;
  ends_at: string;
  total: number;
  done: number;
  percent: number;
};
export type PublicColumn = { name: string; category: string; count: number };
export type PublicView = {
  project_name: string;
  project_key: string;
  sprint: PublicSprint | null;
  columns: PublicColumn[];
};

/** Unauthenticated fetch — no cookies needed. Throws { status } on failure. */
export async function getPublicView(token: string): Promise<PublicView> {
  const res = await fetch(`${API_BASE}/public/status/${encodeURIComponent(token)}`);
  if (!res.ok) throw { status: res.status };
  return res.json();
}

// ── lead controls ──
export type AdminStatus = { enabled: boolean; token: string | null; url: string | null };

export const getPublicStatusAdmin = (key: string) =>
  api<AdminStatus>(`/projects/${encodeURIComponent(key)}/public-status`);
export const enablePublicStatus = (key: string) =>
  api<AdminStatus>(`/projects/${encodeURIComponent(key)}/public-status`, { method: "POST" });
export const disablePublicStatus = (key: string) =>
  api<void>(`/projects/${encodeURIComponent(key)}/public-status`, { method: "DELETE" });
