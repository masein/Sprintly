// The project dashboard's team view: hours by member × day, and each
// member's sprint KPIs (KPI request alongside QA report 6).

import { api } from "./api";

export type MemberKpi = {
  assigned: number;
  completed: number;
  completed_with_due: number;
  on_time: number;
  estimated: number;
};

export type TeamMember = {
  user_id: string;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  avatar_style: string | null;
  avatar_seed: string | null;
  role: "lead" | "contributor" | "watcher";
  /** null: you may not see this person's hours (leads see everyone's). */
  minutes_by_day: number[] | null;
  total_minutes: number | null;
  kpi: MemberKpi;
};

export type Team = {
  from: string;
  to: string;
  days: string[];
  scope: "team" | "self";
  sprint: { id: string; name: string; state: string; snapshot: boolean } | null;
  members: TeamMember[];
  totals_by_day: number[];
};

export const getTeam = (projectKey: string, from: string, to: string) =>
  api<Team>(
    `/projects/${encodeURIComponent(projectKey)}/team?from=${from}&to=${to}`,
  );

export type RangePreset = "this-week" | "last-week" | "this-month" | "last-month";

export const RANGE_PRESETS: { id: RangePreset; label: string }[] = [
  { id: "this-week", label: "this week" },
  { id: "last-week", label: "last week" },
  { id: "this-month", label: "this month" },
  { id: "last-month", label: "last month" },
];

const iso = (d: Date) => d.toISOString().slice(0, 10);

/** A preset as an inclusive [from, to] of UTC dates, relative to `today`. */
export function presetRange(preset: RangePreset, today: Date): { from: string; to: string } {
  const t = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate()));
  const dow = (t.getUTCDay() + 6) % 7; // Monday = 0
  const day = 86_400_000;
  switch (preset) {
    case "this-week": {
      const mon = new Date(t.getTime() - dow * day);
      return { from: iso(mon), to: iso(new Date(mon.getTime() + 6 * day)) };
    }
    case "last-week": {
      const mon = new Date(t.getTime() - (dow + 7) * day);
      return { from: iso(mon), to: iso(new Date(mon.getTime() + 6 * day)) };
    }
    case "this-month": {
      const first = new Date(Date.UTC(t.getUTCFullYear(), t.getUTCMonth(), 1));
      const last = new Date(Date.UTC(t.getUTCFullYear(), t.getUTCMonth() + 1, 0));
      return { from: iso(first), to: iso(last) };
    }
    case "last-month": {
      const first = new Date(Date.UTC(t.getUTCFullYear(), t.getUTCMonth() - 1, 1));
      const last = new Date(Date.UTC(t.getUTCFullYear(), t.getUTCMonth(), 0));
      return { from: iso(first), to: iso(last) };
    }
  }
}

/** n ÷ d as a whole percent, or null when there's no denominator. */
export function rate(n: number, d: number): number | null {
  return d > 0 ? Math.round((n / d) * 100) : null;
}

/** Minutes as "1.5h" / "45m" / "" (empty cell for zero). */
export function cellHours(minutes: number): string {
  if (minutes <= 0) return "";
  if (minutes < 60) return `${minutes}m`;
  const h = minutes / 60;
  return `${Number.isInteger(h) ? h : h.toFixed(1)}h`;
}
