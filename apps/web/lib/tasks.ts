// Task API surface + TanStack Query hooks. Hooks keep the cache keyed so WS
// invalidations land on the right queries.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./api";

export type Task = {
  id: string;
  key: string;                  // e.g. WEB-142
  project_id: string;
  project_key: string;
  board_id: string;
  column_id: string;
  title: string;
  description: string;
  type: "feature" | "bug" | "chore" | "spike" | "incident";
  priority: "p0" | "p1" | "p2" | "p3";
  status: "todo" | "in_progress" | "review" | "done";
  assignee_id: string | null;
  reporter_id: string | null;
  parent_task_id: string | null;
  epic_id: string | null;
  estimate_minutes: number | null;
  story_points: number | null;
  due_date: string | null;
  labels: string[];
  order_in_column: number;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
};

export const listTasks = (projectKey: string, filter?: string) =>
  api<{ items: Task[] }>(
    `/projects/${encodeURIComponent(projectKey)}/tasks` +
      (filter ? `?filter=${encodeURIComponent(filter)}` : ""),
  ).then((r) => r.items);

export const getTask = (taskKey: string) =>
  api<Task>(`/tasks/${encodeURIComponent(taskKey)}`);

export const createTask = (
  projectKey: string,
  body: {
    title: string;
    description?: string;
    column_id?: string;
    type?: Task["type"];
    priority?: Task["priority"];
    assignee_id?: string;
    parent_task_id?: string;
    labels?: string[];
  },
) =>
  api<Task>(`/projects/${encodeURIComponent(projectKey)}/tasks`, {
    method: "POST",
    body,
  });

export const editTask = (taskKey: string, body: Partial<Task>) =>
  api<Task>(`/tasks/${encodeURIComponent(taskKey)}`, {
    method: "PATCH",
    body,
  });

export const deleteTask = (taskKey: string) =>
  api<void>(`/tasks/${encodeURIComponent(taskKey)}`, { method: "DELETE" });

export const moveTask = (
  taskKey: string,
  body: { column_id: string; after_task_id?: string; before_task_id?: string },
) =>
  api<Task>(`/tasks/${encodeURIComponent(taskKey)}/move`, {
    method: "POST",
    body,
  });

// ─── Hooks ──────────────────────────────────────────────────────────────────

export function useTasks(projectKey: string, projectId: string, filter?: string) {
  return useQuery({
    queryKey: ["tasks", projectId, filter ?? null],
    queryFn: () => listTasks(projectKey, filter),
    enabled: !!projectKey && !!projectId,
  });
}

export function useCreateTask(projectKey: string, projectId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: Parameters<typeof createTask>[1]) =>
      createTask(projectKey, input),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["tasks", projectId] });
    },
  });
}

export function useMoveTask(projectId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      taskKey,
      ...body
    }: { taskKey: string; column_id: string; after_task_id?: string; before_task_id?: string }) =>
      moveTask(taskKey, body),
    // Optimistic update inside the cache.
    onMutate: async ({ taskKey, column_id, after_task_id, before_task_id }) => {
      await qc.cancelQueries({ queryKey: ["tasks", projectId] });
      const snapshots = qc.getQueriesData<Task[]>({ queryKey: ["tasks", projectId] });
      for (const [key, list] of snapshots) {
        if (!list) continue;
        qc.setQueryData<Task[]>(key, applyOptimisticMove(list, { taskKey, column_id, after_task_id, before_task_id }));
      }
      return { snapshots };
    },
    onError: (_err, _vars, ctx) => {
      ctx?.snapshots.forEach(([key, data]) => qc.setQueryData<Task[]>(key, data));
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ["tasks", projectId] });
    },
  });
}

// Compute the new list after a move, without contacting the server. Used as
// the optimistic update in useMoveTask. Mirrors the server's resolve_position
// logic closely enough for the UI to feel right; server is still authoritative.
function applyOptimisticMove(
  list: Task[],
  {
    taskKey,
    column_id,
    after_task_id,
    before_task_id,
  }: { taskKey: string; column_id: string; after_task_id?: string; before_task_id?: string },
): Task[] {
  const idx = list.findIndex((t) => t.key === taskKey);
  if (idx < 0) return list;
  const moving = list[idx]!;
  const rest = list.filter((_, i) => i !== idx);

  // Compute a sort_order for the moved card given siblings.
  const siblings = rest
    .filter((t) => t.column_id === column_id)
    .sort((a, b) => a.order_in_column - b.order_in_column);

  let newOrder: number;
  if (after_task_id) {
    const i = siblings.findIndex((t) => t.id === after_task_id);
    if (i < 0) {
      newOrder = (siblings.at(-1)?.order_in_column ?? 0) + 1024;
    } else {
      const next = siblings[i + 1]?.order_in_column;
      newOrder = next === undefined ? siblings[i]!.order_in_column + 1024 : (siblings[i]!.order_in_column + next) / 2;
    }
  } else if (before_task_id) {
    const i = siblings.findIndex((t) => t.id === before_task_id);
    if (i < 0) {
      newOrder = (siblings[0]?.order_in_column ?? 2048) - 1024;
    } else {
      const prev = siblings[i - 1]?.order_in_column;
      newOrder = prev === undefined ? siblings[i]!.order_in_column - 1024 : (prev + siblings[i]!.order_in_column) / 2;
    }
  } else {
    newOrder = (siblings.at(-1)?.order_in_column ?? 0) + 1024;
  }

  return [
    ...rest,
    {
      ...moving,
      column_id,
      order_in_column: newOrder,
    },
  ];
}
