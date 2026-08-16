// Subtask + link API surface.

import { api } from "./api";

export type Subtask = {
  key: string;
  title: string;
  status: "todo" | "in_progress" | "review" | "done";
  assignee_id: string | null;
  estimate_minutes: number | null;
};

export const listSubtasks = (taskKey: string) =>
  api<{ items: Subtask[] }>(`/tasks/${encodeURIComponent(taskKey)}/subtasks`)
    .then((r) => r.items);

/**
 * Convert / promote / reparent: a task key makes this task a subtask of it,
 * null promotes it back to a top-level task. One level deep; the server
 * refuses nesting, cross-project parents, and demoting a task that has
 * subtasks of its own.
 */
export const setTaskParent = (taskKey: string, parentKey: string | null) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}/parent`, {
    method: "PUT",
    body: { parent_key: parentKey },
  });

export type LinkKind = "blocks" | "relates_to" | "duplicates" | "parent_of";

export type Link = {
  kind: LinkKind;
  direction: "incoming" | "outgoing";
  other_task_key: string;
  other_task_title: string;
  other_status: string;
};

export const listLinks = (taskKey: string) =>
  api<{ items: Link[] }>(`/tasks/${encodeURIComponent(taskKey)}/links`)
    .then((r) => r.items);

export const addLink = (taskKey: string, to_task_key: string, kind: LinkKind) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}/links`, {
    method: "POST",
    body: { to_task_key, kind },
  });

export const removeLink = (taskKey: string, to_task_key: string, kind: LinkKind) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}/links`, {
    method: "DELETE",
    body: { to_task_key, kind },
  });
