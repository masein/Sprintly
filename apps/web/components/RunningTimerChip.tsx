"use client";

// The header-pinned running-timer chip. Polls /me/timer every 30s, but also
// re-fetches on TanStack invalidation triggered by timer routes. Shows a
// live mm:ss counter computed from started_at — server clock isn't needed
// for display, just the timestamp.

import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { Square, Timer } from "lucide-react";
import { currentTimer, stopTimer } from "@/lib/timetracking";

export function RunningTimerChip() {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["me-timer"],
    queryFn: () => currentTimer(),
    refetchInterval: 30_000,
  });

  const stop = useMutation({
    mutationFn: () => stopTimer(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["me-timer"] }),
  });

  const running = q.data?.running;
  const [_, setTick] = useState(0);

  // Live tick once per second while running.
  useEffect(() => {
    if (!running) return;
    const i = setInterval(() => setTick((x) => x + 1), 1000);
    return () => clearInterval(i);
  }, [running]);

  if (!running) return null;

  const elapsedSecs = Math.max(
    0,
    Math.floor((Date.now() - new Date(running.started_at).getTime()) / 1000),
  );
  const mm = Math.floor(elapsedSecs / 60);
  const ss = elapsedSecs % 60;
  const label = `${mm.toString().padStart(2, "0")}:${ss.toString().padStart(2, "0")}`;

  return (
    <div
      role="status"
      className="mono flex items-center gap-2 rounded border border-accent/40 bg-accent/10 px-2 py-1 text-xs"
    >
      <Timer size={12} className="text-accent" />
      <Link href={`/tasks/${running.task_key}`} className="text-chrome hover:underline">
        {running.task_key}
      </Link>
      <span className="text-chrome-dim">·</span>
      <span className="text-accent">{label}</span>
      <button
        type="button"
        onClick={() => stop.mutate()}
        disabled={stop.isPending}
        aria-label="Stop timer"
        className="ml-1 text-chrome-dim hover:text-red-300"
      >
        <Square size={12} />
      </button>
    </div>
  );
}
