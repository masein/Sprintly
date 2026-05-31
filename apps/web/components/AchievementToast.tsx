"use client";

// Watches for newly-awarded achievements and shows a toast in the
// bottom-right with confetti. Polls /me/achievements every 60s; remembers
// what it has already shown in localStorage so we don't pop the same toast
// twice across tab navigations.

import { useEffect, useState } from "react";
import { Trophy, X } from "lucide-react";
import { listMyAchievements, type AwardedRow } from "@/lib/achievements";
import { fire } from "@/lib/confetti";

const SEEN_KEY = "sprintly:achievements:seen";

function loadSeen(): Set<string> {
  if (typeof window === "undefined") return new Set();
  try {
    const raw = localStorage.getItem(SEEN_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw) as string[];
    return new Set(arr);
  } catch {
    return new Set();
  }
}

function saveSeen(seen: Set<string>): void {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify(Array.from(seen)));
  } catch {
    /* private mode etc. */
  }
}

export function AchievementToast() {
  const [queue, setQueue] = useState<AwardedRow[]>([]);

  useEffect(() => {
    let alive = true;
    let seen = loadSeen();

    async function poll() {
      try {
        const items = await listMyAchievements();
        if (!alive) return;
        // First load just primes `seen` — we don't pop toasts for things
        // earned before this tab opened (otherwise you'd be confetti-
        // bombed on first page load).
        if (seen.size === 0 && items.length > 0) {
          seen = new Set(items.map((i) => i.code));
          saveSeen(seen);
          return;
        }
        const fresh = items.filter((i) => !seen.has(i.code));
        if (fresh.length > 0) {
          setQueue((q) => [...q, ...fresh]);
          fire(40 * fresh.length);
          for (const f of fresh) seen.add(f.code);
          saveSeen(seen);
        }
      } catch {
        /* ignore; we'll retry next tick */
      }
    }

    void poll();
    const i = window.setInterval(poll, 60_000);
    return () => {
      alive = false;
      window.clearInterval(i);
    };
  }, []);

  if (queue.length === 0) return null;

  return (
    <div
      className="fixed bottom-4 right-4 z-[80] flex w-72 flex-col gap-2"
      role="status"
      aria-live="polite"
    >
      {queue.map((row) => (
        <article
          key={row.code}
          className="rounded-lg border border-accent/40 bg-ink-subtle p-3 shadow-xl"
        >
          <div className="mb-1 flex items-center gap-2">
            <Trophy size={14} className="text-accent" />
            <span className="mono text-[10px] uppercase tracking-widest text-accent">
              achievement unlocked
            </span>
            <button
              type="button"
              onClick={() =>
                setQueue((q) => q.filter((r) => r.code !== row.code))
              }
              className="ml-auto text-chrome-dim hover:text-chrome"
              aria-label="dismiss"
            >
              <X size={12} />
            </button>
          </div>
          <div className="text-sm font-medium text-chrome">{row.title}</div>
          <div className="mono mt-0.5 text-[11px] text-chrome-dim">
            {row.description}
          </div>
        </article>
      ))}
    </div>
  );
}
