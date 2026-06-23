// API wrappers for everything the task detail page needs.

import { api } from "./api";

export type Comment = {
  id: string;
  task_id: string;
  author_id: string | null;
  author_handle: string | null;
  author_avatar_url: string | null;
  author_avatar_style: string | null;
  author_avatar_seed: string | null;
  parent_comment_id: string | null;
  body: string;
  created_at: string;
  edited_at: string | null;
  reactions: ReactionGroup[];
};

export type ReactionGroup = {
  emoji: string;
  count: number;
  user_reacted: boolean;
};

export type Activity = {
  id: string;
  task_id: string;
  actor_id: string | null;
  actor_handle: string | null;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
};

export type Watcher = {
  user_id: string;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  avatar_style: string | null;
  avatar_seed: string | null;
  added_at: string;
};

export type Attachment = {
  id: string;
  task_id: string;
  filename: string;
  mime_type: string;
  size_bytes: number | null;
  status: "pending" | "ready" | "failed";
  download_url: string | null;
  uploader_id: string | null;
  created_at: string;
};

export type AttachmentInit = {
  id: string;
  upload_url: string;
  storage_key: string;
  expires_in: number;
};

// ── Comments ────────────────────────────────────────────────────────────────

export const listComments = (taskKey: string) =>
  api<{ items: Comment[] }>(
    `/tasks/${encodeURIComponent(taskKey)}/comments`,
  ).then((r) => r.items);

export const createComment = (
  taskKey: string,
  body: { body: string; parent_comment_id?: string },
) =>
  api<Comment>(`/tasks/${encodeURIComponent(taskKey)}/comments`, {
    method: "POST",
    body,
  });

export const editComment = (id: string, body: string) =>
  api<Comment>(`/comments/${id}`, { method: "PATCH", body: { body } });

export const deleteComment = (id: string) =>
  api<void>(`/comments/${id}`, { method: "DELETE" });

// ── Reactions ───────────────────────────────────────────────────────────────

export const addReaction = (target: { task_key?: string; comment_id?: string; emoji: string }) =>
  api<void>(`/reactions`, { method: "POST", body: target });

export const removeReaction = (id: string) =>
  api<void>(`/reactions/${id}`, { method: "DELETE" });

// ── Activity ────────────────────────────────────────────────────────────────

export const listActivity = (taskKey: string) =>
  api<{ items: Activity[] }>(
    `/tasks/${encodeURIComponent(taskKey)}/activity`,
  ).then((r) => r.items);

// ── Watchers ────────────────────────────────────────────────────────────────

export const listWatchers = (taskKey: string) =>
  api<{ items: Watcher[] }>(
    `/tasks/${encodeURIComponent(taskKey)}/watchers`,
  ).then((r) => r.items);

export const addWatcher = (taskKey: string, userId: string) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}/watchers`, {
    method: "POST",
    body: { user_id: userId },
  });

export const removeWatcher = (taskKey: string, userId: string) =>
  api<void>(
    `/tasks/${encodeURIComponent(taskKey)}/watchers/${encodeURIComponent(userId)}`,
    { method: "DELETE" },
  );

// ── Attachments ─────────────────────────────────────────────────────────────

export const listAttachments = (taskKey: string) =>
  api<{ items: Attachment[] }>(
    `/tasks/${encodeURIComponent(taskKey)}/attachments`,
  ).then((r) => r.items);

export const initAttachment = (
  taskKey: string,
  body: { filename: string; mime_type: string },
) =>
  api<AttachmentInit>(
    `/tasks/${encodeURIComponent(taskKey)}/attachments`,
    { method: "POST", body },
  );

export const completeAttachment = (
  id: string,
  body: { size_bytes: number; checksum?: string },
) => api<void>(`/attachments/${id}/complete`, { method: "POST", body });

export const deleteAttachment = (id: string) =>
  api<void>(`/attachments/${id}`, { method: "DELETE" });

/**
 * Two-phase upload helper. Hands a presigned PUT to the browser, watches
 * progress via XHR (fetch doesn't expose upload progress yet), then calls
 * the complete endpoint.
 */
export async function uploadAttachment(
  taskKey: string,
  file: File,
  onProgress?: (fraction: number) => void,
): Promise<void> {
  const init = await initAttachment(taskKey, {
    filename: file.name,
    mime_type: file.type || "application/octet-stream",
  });

  await new Promise<void>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("PUT", init.upload_url);
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable && onProgress) onProgress(e.loaded / e.total);
    };
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) resolve();
      else reject(new Error(`upload failed: ${xhr.status}`));
    };
    xhr.onerror = () => reject(new Error("upload network error"));
    if (file.type) xhr.setRequestHeader("Content-Type", file.type);
    xhr.send(file);
  });

  await completeAttachment(init.id, { size_bytes: file.size });
}
