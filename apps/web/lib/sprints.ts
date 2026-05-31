// Sprints + retros API surface.

import { api } from "./api";

export type SprintState = "planned" | "active" | "completed";

export type Sprint = {
  id: string;
  project_id: string;
  project_key: string;
  name: string;
  goal: string;
  starts_at: string;
  ends_at: string;
  state: SprintState;
  velocity_points: number | null;
  summary_md: string | null;
  started_at: string | null;
  completed_at: string | null;
  total_points: number;
  done_points: number;
  task_count: number;
};

export type SprintTask = {
  key: string;
  title: string;
  status: "todo" | "in_progress" | "review" | "done";
  priority: "p0" | "p1" | "p2" | "p3";
  type: string;
  story_points: number | null;
  assignee_id: string | null;
};

export type BurndownPoint = {
  date: string;
  remaining_points: number;
  ideal_points: number;
};

export const listSprints = (projectKey: string) =>
  api<{ items: Sprint[] }>(
    `/projects/${encodeURIComponent(projectKey)}/sprints`,
  ).then((r) => r.items);

export const getSprint = (id: string) => api<Sprint>(`/sprints/${id}`);

export const createSprint = (
  projectKey: string,
  body: { name: string; goal?: string; starts_at: string; ends_at: string },
) =>
  api<Sprint>(`/projects/${encodeURIComponent(projectKey)}/sprints`, {
    method: "POST",
    body,
  });

export const editSprint = (
  id: string,
  body: Partial<Pick<Sprint, "name" | "goal" | "starts_at" | "ends_at">>,
) => api<Sprint>(`/sprints/${id}`, { method: "PATCH", body });

export const startSprint = (id: string) =>
  api<Sprint>(`/sprints/${id}/start`, { method: "POST" });

export const completeSprint = (id: string) =>
  api<Sprint>(`/sprints/${id}/complete`, { method: "POST" });

export const assignTaskToSprint = (id: string, taskKey: string) =>
  api<void>(
    `/sprints/${id}/tasks/${encodeURIComponent(taskKey)}`,
    { method: "POST" },
  );

export const unassignTaskFromSprint = (id: string, taskKey: string) =>
  api<void>(
    `/sprints/${id}/tasks/${encodeURIComponent(taskKey)}`,
    { method: "DELETE" },
  );

export const listSprintTasks = (id: string) =>
  api<{ items: SprintTask[] }>(`/sprints/${id}/tasks`).then((r) => r.items);

export const getBurndown = (id: string) =>
  api<{ items: BurndownPoint[]; total_points: number }>(
    `/sprints/${id}/burndown`,
  );

// ── Retros ──────────────────────────────────────────────────────────────────

export type RetroNote = {
  id: string;
  column_kind: "went_well" | "went_poorly" | "action_item" | "kudos";
  body: string;
  anonymous: boolean;
  author_handle: string | null;
  vote_count: number;
  you_voted: boolean;
  promoted_task_key: string | null;
  created_at: string;
};

export type Retro = {
  id: string;
  sprint_id: string;
  state: "open" | "closed";
  notes: Record<RetroNote["column_kind"], RetroNote[]>;
};

export const getRetro = (sprintId: string) =>
  api<Retro>(`/sprints/${sprintId}/retro`);

export const createNote = (
  retroId: string,
  body: { column_kind: RetroNote["column_kind"]; body: string; anonymous?: boolean },
) => api<void>(`/retros/${retroId}/notes`, { method: "POST", body });

export const editNote = (id: string, body: string) =>
  api<void>(`/retro-notes/${id}`, { method: "PATCH", body: { body } });

export const deleteNote = (id: string) =>
  api<void>(`/retro-notes/${id}`, { method: "DELETE" });

export const voteOnNote = (id: string) =>
  api<void>(`/retro-notes/${id}/vote`, { method: "POST" });

export const unvoteOnNote = (id: string) =>
  api<void>(`/retro-notes/${id}/vote`, { method: "DELETE" });

export const promoteNote = (id: string) =>
  api<{ task_key: string }>(`/retro-notes/${id}/promote`, { method: "POST" });

export const closeRetro = (id: string) =>
  api<{ summary_md: string }>(`/retros/${id}/close`, { method: "POST" });
