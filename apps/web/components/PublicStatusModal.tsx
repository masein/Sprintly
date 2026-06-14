"use client";

// Project lead control for the public status page (F18): toggle it on/off and
// copy the shareable, unauthenticated URL.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, Share2, X } from "lucide-react";
import {
  disablePublicStatus,
  enablePublicStatus,
  getPublicStatusAdmin,
} from "@/lib/publicStatus";
import type { ApiError } from "@/lib/api";

export function PublicStatusModal({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [copied, setCopied] = useState(false);
  const q = useQuery({
    queryKey: ["public-status", projectKey],
    queryFn: () => getPublicStatusAdmin(projectKey),
    retry: false,
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: ["public-status", projectKey] });
  const enable = useMutation({ mutationFn: () => enablePublicStatus(projectKey), onSuccess: invalidate });
  const disable = useMutation({ mutationFn: () => disablePublicStatus(projectKey), onSuccess: invalidate });

  const status = q.data;
  const error = (q.error ?? enable.error ?? disable.error) as unknown as ApiError | undefined;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-md space-y-4 rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <Share2 size={12} /> {projectKey} · public status
            </div>
            <h2 className="text-xl font-semibold">Share a read-only status</h2>
          </div>
          <button type="button" onClick={onClose} className="text-chrome-dim hover:text-chrome" aria-label="Close">
            <X size={18} />
          </button>
        </div>

        <p className="text-xs text-chrome-dim">
          A public link showing the active sprint&apos;s progress and per-column
          counts — no task details, assignees, or anything private. Off by
          default; disabling kills the link.
        </p>

        {status?.enabled && status.url ? (
          <div className="space-y-2">
            <div className="flex items-center gap-2 rounded border border-accent/40 bg-accent/10 p-2">
              <code data-testid="public-url" className="mono flex-1 break-all text-xs text-chrome">{status.url}</code>
              <button
                type="button"
                aria-label="copy link"
                onClick={async () => {
                  await navigator.clipboard.writeText(status.url!);
                  setCopied(true);
                }}
                className="text-chrome-dim hover:text-chrome"
              >
                {copied ? <Check size={14} /> : <Copy size={14} />}
              </button>
            </div>
            <div className="flex items-center gap-2">
              <a
                href={status.url}
                target="_blank"
                rel="noreferrer"
                className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:text-chrome"
              >
                open ↗
              </a>
              <button
                type="button"
                onClick={() => disable.mutate()}
                disabled={disable.isPending}
                className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:border-red-400/40 hover:text-red-300 disabled:opacity-50"
              >
                turn off
              </button>
            </div>
          </div>
        ) : (
          <button
            type="button"
            onClick={() => enable.mutate()}
            disabled={enable.isPending || q.isLoading}
            className="mono rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50"
          >
            {enable.isPending ? "enabling…" : "enable public status"}
          </button>
        )}

        {error && (
          <div className="mono text-[11px] text-red-300">
            {error.status === 403 ? "Only the project lead can do this." : error.message}
          </div>
        )}
      </div>
    </div>
  );
}
