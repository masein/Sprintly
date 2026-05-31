// WebSocket client. Connects to /ws (Caddy proxies → api:8081), authenticates
// implicitly via the access cookie, auto-reconnects with backoff, and routes
// server events into TanStack Query invalidations.
//
// Lifetime: one shared connection per browser tab, owned by a React hook in
// `useRealtime.ts`. Subscribers register listener callbacks; the connection
// itself never lives in React state — only its status.

import type { QueryClient } from "@tanstack/react-query";

export type ServerEvent =
  | { event: "task_created"; data: { project_id: string; board_id: string; task_id: string; key: string } }
  | { event: "task_updated"; data: { project_id: string; task_id: string; key: string } }
  | { event: "task_moved"; data: { project_id: string; board_id: string; task_id: string; key: string; from_column_id: string; to_column_id: string } }
  | { event: "task_deleted"; data: { project_id: string; task_id: string; key: string } }
  | { event: "comment_created"; data: { project_id: string; task_id: string; comment_id: string } }
  | { event: "presence_update"; data: { project_id: string; task_id: string | null; user_id: string; active: boolean } }
  | { event: "notification_created"; data: { user_id: string; notification_id: string } };

type Listener = (e: ServerEvent) => void;

const WS_URL =
  process.env.NEXT_PUBLIC_WS_URL ?? "ws://localhost:8080/ws";

let socket: WebSocket | null = null;
let backoffMs = 1000;
const MAX_BACKOFF = 30_000;
const listeners = new Set<Listener>();
let intentionalClose = false;

export function connectWs(qc: QueryClient): () => void {
  intentionalClose = false;
  open(qc);
  return () => {
    intentionalClose = true;
    if (socket && socket.readyState <= 1) socket.close(1000);
    socket = null;
  };
}

export function subscribe(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function sendPresence(projectId: string, taskId: string | null, active: boolean) {
  if (socket && socket.readyState === 1) {
    socket.send(
      JSON.stringify({ type: "presence", project_id: projectId, task_id: taskId, active }),
    );
  }
}

function open(qc: QueryClient) {
  if (typeof window === "undefined") return;
  try {
    socket = new WebSocket(WS_URL);
  } catch (e) {
    scheduleReconnect(qc);
    return;
  }

  socket.onopen = () => {
    backoffMs = 1000;
  };

  socket.onmessage = (msg) => {
    let parsed: ServerEvent | null = null;
    try {
      parsed = JSON.parse(msg.data) as ServerEvent;
    } catch {
      return;
    }
    routeToQueryCache(parsed, qc);
    for (const fn of listeners) fn(parsed);
  };

  socket.onclose = () => {
    if (!intentionalClose) scheduleReconnect(qc);
  };
  socket.onerror = () => {
    // onclose will follow; backoff happens there.
  };
}

function scheduleReconnect(qc: QueryClient) {
  if (intentionalClose) return;
  const delay = backoffMs;
  backoffMs = Math.min(MAX_BACKOFF, Math.floor(backoffMs * 1.7));
  setTimeout(() => open(qc), delay);
}

// Map server events to query-cache invalidations. The actual UI re-fetches
// only what's relevant; everything else just no-ops.
function routeToQueryCache(e: ServerEvent, qc: QueryClient) {
  switch (e.event) {
    case "task_created":
    case "task_updated":
    case "task_moved":
    case "task_deleted":
      qc.invalidateQueries({ queryKey: ["tasks", e.data.project_id] });
      qc.invalidateQueries({ queryKey: ["task", e.data.key] });
      break;
    case "comment_created":
      qc.invalidateQueries({ queryKey: ["task", e.data.task_id] });
      qc.invalidateQueries({ queryKey: ["task-activity", e.data.task_id] });
      break;
    case "notification_created":
      qc.invalidateQueries({ queryKey: ["notifications"] });
      break;
    default:
      break;
  }
}
