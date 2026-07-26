"use client";

// Week navigation for time-tracking panels: ‹ week of YYYY-MM-DD ›, with a
// "this week" reset once you've stepped away. Weeks are Monday-start, UTC —
// the same convention the timesheets API enforces (period_start must be a
// Monday). Helpers are exported for pages that need the current window.

import { ChevronLeft, ChevronRight } from "lucide-react";

export function thisMondayISO(): string {
  const d = new Date();
  const day = d.getUTCDay(); // 0 = Sun … 6 = Sat
  const offset = day === 0 ? 6 : day - 1;
  d.setUTCDate(d.getUTCDate() - offset);
  return d.toISOString().slice(0, 10);
}

export function shiftMondayISO(mondayISO: string, weeks: number): string {
  const d = new Date(`${mondayISO}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + weeks * 7);
  return d.toISOString().slice(0, 10);
}

/** Sunday of the same week — the inclusive end of a Monday-start window. */
export function sundayOfISO(mondayISO: string): string {
  const d = new Date(`${mondayISO}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + 6);
  return d.toISOString().slice(0, 10);
}

export function WeekNav({
  periodStart,
  onChange,
}: {
  periodStart: string;
  onChange: (nextMondayISO: string) => void;
}) {
  const isCurrent = periodStart === thisMondayISO();
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        aria-label="previous week"
        onClick={() => onChange(shiftMondayISO(periodStart, -1))}
        className="rounded border border-white/10 p-1 text-chrome-dim transition hover:border-white/20 hover:text-chrome"
      >
        <ChevronLeft size={12} />
      </button>
      <span className="mono px-1 text-[11px] text-chrome-dim">
        {isCurrent ? "this week" : `week of ${periodStart}`}
      </span>
      <button
        type="button"
        aria-label="next week"
        onClick={() => onChange(shiftMondayISO(periodStart, 1))}
        disabled={isCurrent}
        className="rounded border border-white/10 p-1 text-chrome-dim transition hover:border-white/20 hover:text-chrome disabled:opacity-40"
      >
        <ChevronRight size={12} />
      </button>
      {!isCurrent && (
        <button
          type="button"
          onClick={() => onChange(thisMondayISO())}
          className="mono ml-1 text-[11px] text-accent hover:underline"
        >
          this week
        </button>
      )}
    </div>
  );
}
