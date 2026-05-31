"use client";

// Coffee meter. Fills with the user's own time-logged-today (mostly a wink;
// per spec it's "for the user, not for their manager"). Configurable off in
// settings via `settings.coffee_meter = false`.
//
// Thresholds → tooltip vibes:
//   <  2h   →  "Just getting started."
//   <  6h   →  "Steady."
//   <  9h   →  "You okay?"
//   <  11h  →  "Sprintly suggests a walk."
//   >= 11h  →  "Genuinely, log off."

import { useQuery } from "@tanstack/react-query";
import { Coffee } from "lucide-react";
import { me } from "@/lib/auth-bundle";
import { getMyDashboard } from "@/lib/dashboards";

const HOUR = 60;
const MAX_MINUTES = 12 * HOUR;

function vibe(minutes: number): string {
  const h = minutes / HOUR;
  if (h < 2) return "Just getting started.";
  if (h < 6) return "Steady.";
  if (h < 9) return `${Math.round(h)} espressos in. You okay?`;
  if (h < 11) return "Sprintly suggests a walk.";
  return "Genuinely, log off.";
}

export function CoffeeMeter() {
  const meQ = useQuery({ queryKey: ["me"], queryFn: () => me() });
  const d = useQuery({
    queryKey: ["my-dashboard"],
    queryFn: () => getMyDashboard(),
    refetchInterval: 120_000,
  });

  const enabled = (meQ.data?.settings as { coffee_meter?: boolean } | undefined)
    ?.coffee_meter !== false;
  if (!enabled) return null;

  const minutes = d.data?.time_this_week_minutes ?? 0;
  const pct = Math.min(100, (minutes / MAX_MINUTES) * 100);
  const tooltip = vibe(minutes);

  return (
    <div
      className="mono group relative flex items-center gap-1.5 rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome-dim"
      title={tooltip}
    >
      <Coffee size={12} className="text-accent" />
      <div className="h-1.5 w-16 overflow-hidden rounded-full bg-ink">
        <div
          className="h-full bg-accent transition-all"
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className="hidden text-[10px] md:inline">
        {(minutes / HOUR).toFixed(1)}h
      </span>
    </div>
  );
}
