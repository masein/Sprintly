"use client";

// Manage a project's outbound webhooks: connect a generic signed endpoint or a
// Slack/Discord target, pick events, send a test, and inspect recent delivery
// attempts. Opened from the project header (lead-only).

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleX, Send, Trash2, X } from "lucide-react";
import {
  createWebhook,
  deleteWebhook,
  listDeliveries,
  listWebhooks,
  sendTestWebhook,
  updateWebhook,
  WEBHOOK_EVENTS,
  type Webhook,
  type WebhookTarget,
} from "@/lib/webhooks";
import type { ApiError } from "@/lib/api";

const TARGETS: { value: WebhookTarget; label: string; hint: string }[] = [
  { value: "outbound", label: "generic", hint: "signed JSON to any URL" },
  { value: "slack", label: "slack", hint: "incoming-webhook URL" },
  { value: "discord", label: "discord", hint: "webhook URL" },
];

export function WebhooksManager({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["webhooks", projectKey],
    queryFn: () => listWebhooks(projectKey),
    retry: false,
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: ["webhooks", projectKey] });

  const [target, setTarget] = useState<WebhookTarget>("outbound");
  const [url, setUrl] = useState("");
  const [secret, setSecret] = useState("");
  const [events, setEvents] = useState<string[]>(["task.created", "task.moved"]);
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () =>
      createWebhook(projectKey, {
        url: url.trim(),
        target_type: target,
        events,
        secret: target === "outbound" ? secret : undefined,
      }),
    onSuccess: () => {
      setUrl("");
      setSecret("");
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const toggle = useMutation({
    mutationFn: (v: { id: string; active: boolean }) =>
      updateWebhook(v.id, { active: v.active }),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteWebhook(id),
    onSuccess: invalidate,
  });

  const hooks = q.data ?? [];
  const canSubmit = url.trim().length > 0 && events.length > 0 && (target !== "outbound" || secret.length >= 8);

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="max-h-[90vh] w-full max-w-2xl space-y-4 overflow-y-auto rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              {projectKey} · webhooks
            </div>
            <h2 className="text-xl font-semibold">Outbound webhooks</h2>
            <p className="mt-1 text-xs text-chrome-dim">
              Fire on board events. Generic targets get signed JSON
              (<span className="mono">X-Sprintly-Signature</span>); Slack and
              Discord get a formatted message.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        <ul className="space-y-1">
          {hooks.map((h) => (
            <WebhookRow
              key={h.id}
              hook={h}
              onToggle={(active) => toggle.mutate({ id: h.id, active })}
              onTest={() => sendTestWebhook(h.id)}
              onDelete={() => {
                if (confirm(`Delete this ${h.target_type} webhook? No undo.`)) remove.mutate(h.id);
              }}
            />
          ))}
          {hooks.length === 0 && (
            <li className="mono text-[11px] text-chrome-dim">
              no webhooks — the outside world hears nothing
            </li>
          )}
        </ul>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (canSubmit) add.mutate();
          }}
          className="space-y-2 border-t border-white/10 pt-3"
        >
          <div className="flex items-center gap-2">
            <select
              value={target}
              onChange={(e) => setTarget(e.target.value as WebhookTarget)}
              aria-label="target type"
              className="mono rounded border border-white/10 bg-ink px-1.5 py-1 text-xs text-chrome"
            >
              {TARGETS.map((t) => (
                <option key={t.value} value={t.value}>{t.label}</option>
              ))}
            </select>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder={TARGETS.find((t) => t.value === target)?.hint}
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <button
              type="submit"
              disabled={add.isPending || !canSubmit}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              add
            </button>
          </div>
          {target === "outbound" && (
            <input
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
              type="password"
              placeholder="signing secret (≥ 8 chars)"
              className="mono w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
          )}
          <div className="flex flex-wrap gap-1.5">
            {WEBHOOK_EVENTS.map((ev) => {
              const on = events.includes(ev);
              return (
                <button
                  key={ev}
                  type="button"
                  onClick={() =>
                    setEvents((prev) =>
                      on ? prev.filter((e) => e !== ev) : [...prev, ev],
                    )
                  }
                  className={`mono rounded border px-1.5 py-0.5 text-[10px] ${
                    on
                      ? "border-accent/50 bg-accent/10 text-chrome"
                      : "border-white/10 text-chrome-dim hover:text-chrome"
                  }`}
                >
                  {ev}
                </button>
              );
            })}
          </div>
          {error && <div className="mono text-[11px] text-red-300">{error}</div>}
        </form>
      </div>
    </div>
  );
}

function WebhookRow({
  hook,
  onToggle,
  onTest,
  onDelete,
}: {
  hook: Webhook;
  onToggle: (active: boolean) => void;
  onTest: () => Promise<void>;
  onDelete: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [tested, setTested] = useState(false);
  const deliveries = useQuery({
    queryKey: ["webhook-deliveries", hook.id],
    queryFn: () => listDeliveries(hook.id),
    enabled: open,
    refetchInterval: open ? 5_000 : false,
  });

  return (
    <li className="rounded border border-white/10">
      <div className="flex items-center gap-2 px-2 py-1.5">
        <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim">
          {hook.target_type}
        </span>
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="mono min-w-0 flex-1 truncate text-left text-xs text-chrome hover:text-accent"
          title={hook.url}
        >
          {hook.url}
        </button>
        {hook.last_status != null && (
          <span
            className={`mono inline-flex items-center gap-1 text-[10px] ${
              hook.last_status >= 200 && hook.last_status < 300
                ? "text-emerald-300"
                : "text-red-300"
            }`}
          >
            {hook.last_status >= 200 && hook.last_status < 300 ? (
              <CircleCheck size={10} />
            ) : (
              <CircleX size={10} />
            )}
            {hook.last_status}
          </span>
        )}
        <label className="mono flex items-center gap-1 text-[10px] text-chrome-dim">
          <input
            type="checkbox"
            checked={hook.active}
            onChange={(e) => onToggle(e.target.checked)}
          />
          active
        </label>
        <button
          type="button"
          aria-label="send test"
          title="send a test delivery"
          onClick={() => {
            setTested(true);
            setOpen(true);
            onTest().finally(() => setTimeout(() => setTested(false), 1500));
          }}
          className="text-chrome-dim hover:text-chrome"
        >
          <Send size={12} />
        </button>
        <button
          type="button"
          aria-label="delete webhook"
          onClick={onDelete}
          className="text-chrome-dim hover:text-red-300"
        >
          <Trash2 size={13} />
        </button>
      </div>

      <div className="mono px-2 pb-1 text-[10px] text-chrome-dim">
        {hook.events.join(" · ") || "no events"}
        {tested && <span className="ml-2 text-accent">test queued…</span>}
      </div>

      {open && (
        <div className="border-t border-white/10 px-2 py-1.5">
          <div className="mono mb-1 text-[10px] uppercase tracking-widest text-chrome-dim">
            recent deliveries
          </div>
          <ul className="space-y-0.5">
            {(deliveries.data ?? []).map((d) => (
              <li key={d.id} className="mono flex items-center gap-2 text-[10px]">
                {d.ok ? (
                  <CircleCheck size={9} className="text-emerald-300" />
                ) : (
                  <CircleX size={9} className="text-red-300" />
                )}
                <span className="text-chrome-dim">
                  {d.created_at.slice(11, 19)}
                </span>
                <span className="text-chrome">{d.event}</span>
                <span className="text-chrome-dim">
                  {d.status_code ?? "—"}
                  {d.attempt > 1 ? ` · try ${d.attempt}` : ""}
                </span>
                {d.error && (
                  <span className="ml-auto truncate text-red-300">{d.error}</span>
                )}
              </li>
            ))}
            {deliveries.data?.length === 0 && (
              <li className="mono text-[10px] text-chrome-dim">no deliveries yet</li>
            )}
          </ul>
        </div>
      )}
    </li>
  );
}
