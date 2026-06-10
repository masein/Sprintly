// Per-project label registry (name → colour).

import { api } from "./api";

export type Label = {
  id: string;
  project_id: string;
  name: string;
  color: string;
  created_at: string;
};

export const listProjectLabels = (key: string) =>
  api<Label[]>(`/projects/${encodeURIComponent(key)}/labels`);

export const createLabel = (key: string, name: string, color: string) =>
  api<Label>(`/projects/${encodeURIComponent(key)}/labels`, {
    method: "POST",
    body: { name, color },
  });

export const updateLabel = (id: string, body: { name?: string; color?: string }) =>
  api<Label>(`/labels/${id}`, { method: "PATCH", body });

export const deleteLabel = (id: string) =>
  api<void>(`/labels/${id}`, { method: "DELETE" });

/** Lowercased name → colour, for tinting free-form label chips. */
export function labelColorMap(labels: Label[]): Record<string, string> {
  const m: Record<string, string> = {};
  for (const l of labels) m[l.name.toLowerCase()] = l.color;
  return m;
}
