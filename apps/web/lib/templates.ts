// Task templates + backlog + bulk ops (F9).

import { api } from "./api";

export type Recurrence = "none" | "daily" | "weekly" | "monthly";
export type TaskType = "feature" | "bug" | "chore" | "spike" | "incident";
export type Priority = "p0" | "p1" | "p2" | "p3";

export type Template = {
  id: string;
  project_id: string;
  name: string;
  title: string;
  description: string;
  type: TaskType;
  priority: Priority;
  labels: string[];
  recurrence: Recurrence;
  next_run_at: string | null;
  created_at: string;
};

export type BacklogItem = {
  id: string;
  key: string;
  title: string;
  priority: Priority;
  status: "todo" | "in_progress" | "review" | "done";
  assignee_id: string | null;
  labels: string[];
};

export type BulkOp =
  | { op: "assign"; assignee_id: string | null }
  | { op: "sprint"; sprint_id: string | null }
  | { op: "column"; column_id: string }
  | { op: "label"; labels: string[] }
  | { op: "delete" };

export const listTemplates = (key: string) =>
  api<Template[]>(`/projects/${encodeURIComponent(key)}/templates`);

export const createTemplate = (
  key: string,
  body: {
    name: string;
    title: string;
    description?: string;
    type?: TaskType;
    priority?: Priority;
    labels?: string[];
    recurrence?: Recurrence;
  },
) => api<Template>(`/projects/${encodeURIComponent(key)}/templates`, { method: "POST", body });

export const updateTemplate = (
  id: string,
  body: Partial<{ name: string; title: string; recurrence: Recurrence; priority: Priority }>,
) => api<Template>(`/templates/${id}`, { method: "PATCH", body });

export const deleteTemplate = (id: string) =>
  api<void>(`/templates/${id}`, { method: "DELETE" });

export const instantiateTemplate = (id: string, column_id?: string) =>
  api<{ key: string }>(`/templates/${id}/instantiate`, {
    method: "POST",
    body: column_id ? { column_id } : {},
  });

export const listBacklog = (key: string) =>
  api<BacklogItem[]>(`/projects/${encodeURIComponent(key)}/backlog`);

export const bulkTasks = (key: string, task_keys: string[], op: BulkOp) =>
  api<{ affected: number }>(`/projects/${encodeURIComponent(key)}/tasks/bulk`, {
    method: "POST",
    body: { task_keys, ...op },
  });
