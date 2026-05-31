// Search API surface used by the cmd-K palette and /me/tasks page.

import { api } from "./api";

export type SearchHits = {
  tasks: { key: string; project_key: string; title: string; status: string; priority: string; type: string }[];
  projects: { key: string; name: string; color: string; icon: string }[];
  users: { id: string; handle: string; display_name: string }[];
};

export const search = (q: string, limit = 8) =>
  api<SearchHits>(`/search?q=${encodeURIComponent(q)}&limit=${limit}`);

export type MyTask = {
  key: string;
  project_key: string;
  title: string;
  status: "todo" | "in_progress" | "review" | "done";
  priority: "p0" | "p1" | "p2" | "p3";
  type: string;
  due_date: string | null;
  updated_at: string;
};

export const myTasks = () =>
  api<{ items: MyTask[] }>("/me/tasks").then((r) => r.items);
