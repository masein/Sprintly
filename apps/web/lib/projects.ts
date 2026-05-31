// Project + board API surface. Thin wrappers around api().

import { api } from "./api";

export type Project = {
  id: string;
  key: string;
  name: string;
  description: string;
  icon: string;
  color: string;
  archived_at: string | null;
  settings: Record<string, unknown>;
  member_count: number;
  your_role: "lead" | "contributor" | "watcher" | null;
  created_at: string;
};

export type Board = {
  id: string;
  project_id: string;
  name: string;
  type: "kanban" | "sprint";
  is_default: boolean;
  columns: Column[];
  created_at: string;
};

export type Column = {
  id: string;
  board_id: string;
  name: string;
  category: "todo" | "in_progress" | "review" | "done";
  wip_limit: number | null;
  sort_order: number;
};

export type Member = {
  user_id: string;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  role: "lead" | "contributor" | "watcher";
  added_at: string;
};

// ── projects ────────────────────────────────────────────────────────────────

export const listProjects = () =>
  api<{ items: Project[] }>("/projects").then((r) => r.items);

export const getProject = (key: string) =>
  api<Project>(`/projects/${encodeURIComponent(key)}`);

export const createProject = (p: {
  key: string;
  name: string;
  description?: string;
  icon?: string;
  color?: string;
}) => api<Project>("/projects", { method: "POST", body: p });

export const editProject = (
  key: string,
  p: Partial<Pick<Project, "name" | "description" | "icon" | "color">>,
) =>
  api<Project>(`/projects/${encodeURIComponent(key)}`, {
    method: "PATCH",
    body: p,
  });

export const archiveProject = (key: string) =>
  api<void>(`/projects/${encodeURIComponent(key)}/archive`, { method: "POST" });

export const unarchiveProject = (key: string) =>
  api<void>(`/projects/${encodeURIComponent(key)}/unarchive`, {
    method: "POST",
  });

// ── members ─────────────────────────────────────────────────────────────────

export const listMembers = (key: string) =>
  api<{ items: Member[] }>(`/projects/${encodeURIComponent(key)}/members`).then(
    (r) => r.items,
  );

export const addMember = (key: string, body: { user_id: string; role?: string }) =>
  api<void>(`/projects/${encodeURIComponent(key)}/members`, {
    method: "POST",
    body,
  });

export const removeMember = (key: string, userId: string) =>
  api<void>(
    `/projects/${encodeURIComponent(key)}/members/${encodeURIComponent(userId)}`,
    { method: "DELETE" },
  );

// ── boards / columns ────────────────────────────────────────────────────────

export const listBoards = (key: string) =>
  api<{ items: Board[] }>(`/projects/${encodeURIComponent(key)}/boards`).then(
    (r) => r.items,
  );

export const getBoard = (boardId: string) =>
  api<Board>(`/boards/${boardId}`);

export const createColumn = (
  boardId: string,
  body: { name: string; category: Column["category"]; wip_limit?: number; after_column_id?: string },
) => api<Column>(`/boards/${boardId}/columns`, { method: "POST", body });

export const editColumn = (
  columnId: string,
  body: Partial<Pick<Column, "name" | "category" | "wip_limit">>,
) => api<Column>(`/columns/${columnId}`, { method: "PATCH", body });

export const deleteColumn = (columnId: string) =>
  api<void>(`/columns/${columnId}`, { method: "DELETE" });

export const reorderColumns = (boardId: string, order: string[]) =>
  api<void>(`/boards/${boardId}/columns/reorder`, {
    method: "POST",
    body: { order },
  });
