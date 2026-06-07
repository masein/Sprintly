// In-app notification center API.

import { api } from "./api";

export type Notification = {
  id: string;
  kind: "mention" | "assigned" | "comment";
  title: string;
  body: string | null;
  link: string | null;
  actor_handle: string | null;
  read_at: string | null;
  created_at: string;
};

export const listNotifications = () => api<Notification[]>("/me/notifications");

export const unreadCount = () =>
  api<{ count: number }>("/me/notifications/unread-count");

export const markRead = (id: string) =>
  api<void>(`/me/notifications/${id}/read`, { method: "POST" });

export const markAllRead = () =>
  api<void>("/me/notifications/read-all", { method: "POST" });
