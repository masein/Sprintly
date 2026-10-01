// Roadmap (F6): epics (date-ranged groupings with done/total progress) and
// milestones (dated targets). Tasks belong to at most one epic.

import { api } from "./api";

export type Epic = {
  id: string;
  project_id: string;
  name: string;
  color: string;
  start_date: string | null; // YYYY-MM-DD
  end_date: string | null;
  created_at: string;
  task_count: number;
  done_count: number;
};

/** A task inside an epic, as the roadmap's expanded epic lists it. */
export type EpicTask = {
  key: string;
  title: string;
  status: "todo" | "in_progress" | "review" | "done";
  priority: "p0" | "p1" | "p2" | "p3";
  type: string;
  story_points: number | null;
  assignee_handle: string | null;
  parent_key: string | null;
};

export const listEpicTasks = (epicId: string) =>
  api<{ items: EpicTask[] }>(`/epics/${encodeURIComponent(epicId)}/tasks`).then((r) => r.items);

export type Milestone = {
  id: string;
  project_id: string;
  name: string;
  due_date: string; // YYYY-MM-DD
  created_at: string;
};

export const listEpics = (key: string) =>
  api<Epic[]>(`/projects/${encodeURIComponent(key)}/epics`);

export const createEpic = (
  key: string,
  body: { name: string; color?: string; start_date?: string | null; end_date?: string | null },
) => api<Epic>(`/projects/${encodeURIComponent(key)}/epics`, { method: "POST", body });

export const updateEpic = (
  id: string,
  body: Partial<{ name: string; color: string; start_date: string | null; end_date: string | null }>,
) => api<Epic>(`/epics/${id}`, { method: "PATCH", body });

export const deleteEpic = (id: string) =>
  api<void>(`/epics/${id}`, { method: "DELETE" });

export const listMilestones = (key: string) =>
  api<Milestone[]>(`/projects/${encodeURIComponent(key)}/milestones`);

export const createMilestone = (key: string, body: { name: string; due_date: string }) =>
  api<Milestone>(`/projects/${encodeURIComponent(key)}/milestones`, { method: "POST", body });

export const updateMilestone = (
  id: string,
  body: Partial<{ name: string; due_date: string }>,
) => api<Milestone>(`/milestones/${id}`, { method: "PATCH", body });

export const deleteMilestone = (id: string) =>
  api<void>(`/milestones/${id}`, { method: "DELETE" });

export const assignTaskEpic = (taskKey: string, epic_id: string | null) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}/epic`, {
    method: "PUT",
    body: { epic_id },
  });
