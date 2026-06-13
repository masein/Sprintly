// Saved board views (F8): a named filter + swimlane grouping, private or
// shared. `filter` is the chip array the board already uses — opaque to the
// backend, restored verbatim on reopen.

import { api } from "./api";
import type { Chip } from "@/components/BoardFilters";

export type GroupBy = "none" | "assignee" | "label" | "priority";

export type BoardView = {
  id: string;
  project_id: string;
  owner_id: string;
  name: string;
  filter: Chip[];
  group_by: GroupBy;
  shared: boolean;
  created_at: string;
  is_mine: boolean;
};

export const listBoardViews = (key: string) =>
  api<BoardView[]>(`/projects/${encodeURIComponent(key)}/board-views`);

export const createBoardView = (
  key: string,
  body: { name: string; filter: Chip[]; group_by: GroupBy; shared: boolean },
) =>
  api<BoardView>(`/projects/${encodeURIComponent(key)}/board-views`, {
    method: "POST",
    body,
  });

export const updateBoardView = (
  id: string,
  body: Partial<{ name: string; filter: Chip[]; group_by: GroupBy; shared: boolean }>,
) => api<BoardView>(`/board-views/${id}`, { method: "PATCH", body });

export const deleteBoardView = (id: string) =>
  api<void>(`/board-views/${id}`, { method: "DELETE" });
