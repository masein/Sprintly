"use client";

// Renders toasts from lib/toast. Mounted once in Providers, styled after
// AchievementToast so the two stacks look related (this one sits bottom-left
// to stay out of its way).

import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { dismissToast, subscribeToasts, type Toast } from "@/lib/toast";

export function Toaster() {
  const [toasts, setToasts] = useState<Toast[]>([]);

  useEffect(() => subscribeToasts(setToasts), []);

  if (toasts.length === 0) return null;

  return (
    <div
      className="fixed bottom-4 left-4 z-[80] flex w-80 flex-col gap-2"
      role="status"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className="flex items-center gap-2 rounded border border-white/10 bg-ink p-3 shadow-xl"
        >
          <span className="mono min-w-0 flex-1 truncate text-xs text-chrome" title={t.message}>
            {t.message}
          </span>
          {t.actionLabel && (
            <button
              type="button"
              onClick={() => {
                dismissToast(t.id);
                void t.onAction?.();
              }}
              className="mono shrink-0 text-xs text-accent hover:underline"
            >
              {t.actionLabel}
            </button>
          )}
          <button
            type="button"
            aria-label="dismiss"
            onClick={() => dismissToast(t.id)}
            className="shrink-0 text-chrome-dim hover:text-chrome"
          >
            <X size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
