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
  avatar_style: string | null;
  avatar_seed: string | null;
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
  p: Partial<Pick<Project, "name" | "description" | "icon" | "color">> & {
    /** Renames the project key AND rewrites every task key (TST-12 → OPS-12).
     *  Old URLs stop resolving — warn before sending. */
    key?: string;
  },
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

// ── project documents ───────────────────────────────────────────────────────

export type ProjectDocument = {
  id: string;
  filename: string;
  mime_type: string;
  size_bytes: number | null;
  status: "pending" | "ready" | "failed";
  uploader_handle: string | null;
  created_at: string;
  /** Stable same-origin link; re-signs at click time. null until uploaded. */
  download_url: string | null;
};

export const listProjectDocuments = (key: string) =>
  api<{ items: ProjectDocument[] }>(`/projects/${encodeURIComponent(key)}/documents`).then(
    (r) => r.items,
  );

export const deleteProjectDocument = (id: string) =>
  api<void>(`/project-documents/${encodeURIComponent(id)}`, { method: "DELETE" });

/** Same two-phase presigned upload as task attachments. */
export async function uploadProjectDocument(
  key: string,
  file: File,
  onProgress?: (fraction: number) => void,
): Promise<void> {
  const init = await api<{ id: string; upload_url: string }>(
    `/projects/${encodeURIComponent(key)}/documents`,
    {
      method: "POST",
      body: { filename: file.name, mime_type: file.type || "application/octet-stream" },
    },
  );
  try {
    await new Promise<void>((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open("PUT", init.upload_url);
      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable && onProgress) onProgress(e.loaded / e.total);
      };
      xhr.onload = () =>
        xhr.status >= 200 && xhr.status < 300
          ? resolve()
          : reject(new Error(`storage refused the upload (${xhr.status})`));
      xhr.onerror = () => reject(new Error("the upload didn't reach storage"));
      if (file.type) xhr.setRequestHeader("Content-Type", file.type);
      xhr.send(file);
    });
  } catch (e) {
    await deleteProjectDocument(init.id).catch(() => {});
    throw e;
  }
  await api<void>(`/project-documents/${encodeURIComponent(init.id)}/complete`, {
    method: "POST",
    body: { size_bytes: file.size },
  });
}

export type DeletedProject = {
  id: string;
  key: string;
  name: string;
  deleted_at: string;
  task_count: number;
  key_taken: boolean;
};

export const listDeletedProjects = () =>
  api<{ items: DeletedProject[] }>("/admin/deleted-projects").then((r) => r.items);

export const restoreProject = (id: string) =>
  api<{ key: string; tasks: number }>(
    `/admin/deleted-projects/${encodeURIComponent(id)}/restore`,
    { method: "POST" },
  );

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

export const changeMemberRole = (key: string, userId: string, role: Member["role"]) =>
  api<void>(
    `/projects/${encodeURIComponent(key)}/members/${encodeURIComponent(userId)}`,
    { method: "PATCH", body: { role } },
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
