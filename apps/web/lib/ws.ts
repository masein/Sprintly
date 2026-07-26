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

// NEXT_PUBLIC_WS_URL is inlined at build time, but a prod image is built once
// in CI and pulled onto whatever host the operator runs it on — so we can't
// bake an absolute ws:// host there. When the configured value is absolute
// (dev: `ws://localhost:8080/ws`) we use it verbatim; otherwise we treat it as
// a path ("/ws" by default) and resolve scheme+host from the page origin at
// connect time. Same-origin behind the reverse proxy, so this "just works"
// over both http/ws and https/wss without a rebuild.
function resolveWsUrl(): string {
  const configured = process.env.NEXT_PUBLIC_WS_URL;
  if (configured && /^wss?:\/\//i.test(configured)) return configured;
  const path = configured && configured.startsWith("/") ? configured : "/ws";
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}${path}`;
}

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
    socket = new WebSocket(resolveWsUrl());
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
      // Overview surfaces fold task state into aggregates — refresh the ones
      // that are actually mounted (invalidate no-ops for inactive keys).
      // "My day" was the reported offender: it sat stale until a manual
      // reload because nothing ever told it the world had changed.
      qc.invalidateQueries({ queryKey: ["my-dashboard"] });
      qc.invalidateQueries({ queryKey: ["project-dashboard"] });
      qc.invalidateQueries({ queryKey: ["my-tasks"] });
      break;
    case "comment_created":
      qc.invalidateQueries({ queryKey: ["task", e.data.task_id] });
      qc.invalidateQueries({ queryKey: ["task-activity", e.data.task_id] });
      break;
    case "notification_created":
      qc.invalidateQueries({ queryKey: ["notifications"] });
      qc.invalidateQueries({ queryKey: ["my-dashboard"] });
      break;
    default:
      break;
  }
}
