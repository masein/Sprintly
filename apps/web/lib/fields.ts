// Per-project custom fields (definitions) + per-task values.

import { api } from "./api";

export type FieldType = "text" | "number" | "select" | "date";

export type CustomField = {
  id: string;
  project_id: string;
  name: string;
  type: FieldType;
  options: string[];
  created_at: string;
};

export type TaskFieldValue = {
  field_id: string;
  name: string;
  type: FieldType;
  options: string[];
  value: string | null;
};

export const listProjectFields = (key: string) =>
  api<CustomField[]>(`/projects/${encodeURIComponent(key)}/fields`);

export const createField = (
  key: string,
  body: { name: string; type: FieldType; options?: string[] },
) =>
  api<CustomField>(`/projects/${encodeURIComponent(key)}/fields`, {
    method: "POST",
    body,
  });

export const updateField = (
  id: string,
  body: { name?: string; options?: string[] },
) => api<CustomField>(`/fields/${id}`, { method: "PATCH", body });

export const deleteField = (id: string) =>
  api<void>(`/fields/${id}`, { method: "DELETE" });

export const listTaskFieldValues = (taskKey: string) =>
  api<TaskFieldValue[]>(`/tasks/${encodeURIComponent(taskKey)}/fields`);

export const setTaskFieldValue = (taskKey: string, fieldId: string, value: string) =>
  api<{ field_id: string; value: string }>(
    `/tasks/${encodeURIComponent(taskKey)}/fields/${fieldId}`,
    { method: "PUT", body: { value } },
  );

export const clearTaskFieldValue = (taskKey: string, fieldId: string) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}/fields/${fieldId}`, {
    method: "DELETE",
  });
