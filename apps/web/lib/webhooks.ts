// Per-project outbound webhooks (F2). A webhook is a generic signed endpoint
// ("outbound") or a Slack/Discord target that gets a formatted message.

import { api } from "./api";

export type WebhookTarget = "outbound" | "slack" | "discord";

/// Events a webhook can subscribe to (mirrors domain::webhooks::EVENTS).
export const WEBHOOK_EVENTS = [
  "task.created",
  "task.updated",
  "task.moved",
  "task.deleted",
  "comment.created",
] as const;

export type Webhook = {
  id: string;
  project_id: string;
  url: string;
  target_type: WebhookTarget;
  events: string[];
  active: boolean;
  last_delivery_at: string | null;
  last_status: number | null;
  created_at: string;
};

export type WebhookDelivery = {
  id: string;
  event: string;
  status_code: number | null;
  ok: boolean;
  error: string | null;
  attempt: number;
  created_at: string;
};

export const listWebhooks = (key: string) =>
  api<{ items: Webhook[] }>(`/projects/${encodeURIComponent(key)}/webhooks`).then(
    (r) => r.items,
  );

export const createWebhook = (
  key: string,
  body: {
    url: string;
    target_type: WebhookTarget;
    events: string[];
    secret?: string;
  },
) =>
  api<Webhook>(`/projects/${encodeURIComponent(key)}/webhooks`, {
    method: "POST",
    body,
  });

export const updateWebhook = (
  id: string,
  body: { url?: string; events?: string[]; active?: boolean; secret?: string },
) => api<Webhook>(`/webhooks/${id}`, { method: "PATCH", body });

export const deleteWebhook = (id: string) =>
  api<void>(`/webhooks/${id}`, { method: "DELETE" });

export const listDeliveries = (id: string) =>
  api<{ items: WebhookDelivery[] }>(`/webhooks/${id}/deliveries`).then((r) => r.items);

export const sendTestWebhook = (id: string) =>
  api<void>(`/webhooks/${id}/test`, { method: "POST" });
