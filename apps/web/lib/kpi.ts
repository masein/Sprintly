// KPI arithmetic shared by the dashboard and sprint page — kept pure so the
// edge cases (no sprints yet, zero baselines) are unit-tested, not eyeballed.

import type { VelocitySprint } from "./sprints";

export type VelocityUnit = "points" | "tasks" | "hours" | "logged";

export const VELOCITY_UNITS: { unit: VelocityUnit; label: string; suffix: string }[] = [
  { unit: "points", label: "points", suffix: "pts" },
  { unit: "tasks", label: "tasks", suffix: "tasks" },
  { unit: "hours", label: "est. hours", suffix: "h" },
  { unit: "logged", label: "logged hours", suffix: "h" },
];

/** One sprint's velocity in the chosen unit. Hours to one decimal. */
export function velocityValue(s: VelocitySprint, unit: VelocityUnit): number {
  switch (unit) {
    case "points":
      return s.points;
    case "tasks":
      return s.tasks;
    case "hours":
      return Math.round((s.estimate_minutes / 60) * 10) / 10;
    case "logged":
      return Math.round((s.logged_minutes / 60) * 10) / 10;
  }
}

export type VelocitySummary = {
  /** The running sprint so far, else the last completed one. */
  current: number | null;
  currentLabel: string | null;
  /** Mean over completed sprints — the KPI doc's "sum ÷ number of sprints". */
  average: number | null;
  /** Last completed vs the one before, in percent. */
  vsPrevious: number | null;
};

export function summarizeVelocity(
  done: VelocitySprint[],
  running: VelocitySprint | null,
  unit: VelocityUnit,
): VelocitySummary {
  const values = done.map((s) => velocityValue(s, unit));
  const average =
    values.length > 0
      ? Math.round((values.reduce((a, b) => a + b, 0) / values.length) * 10) / 10
      : null;
  const last = values.at(-1);
  const prev = values.at(-2);
  const vsPrevious =
    last !== undefined && prev !== undefined && prev > 0
      ? Math.round(((last - prev) / prev) * 100)
      : null;
  const cur = running ?? done.at(-1) ?? null;
  return {
    current: cur ? velocityValue(cur, unit) : null,
    currentLabel: cur ? (running ? `${cur.name} (so far)` : cur.name) : null,
    average,
    vsPrevious,
  };
}

/** done ÷ total as a whole percent; 0 when there's nothing to do. */
export function percent(done: number, total: number): number {
  return total > 0 ? Math.round((done / total) * 100) : 0;
}

/** "+12%", "−5%", "±0%". */
export function signedPercent(n: number): string {
  if (n > 0) return `+${n}%`;
  if (n < 0) return `−${Math.abs(n)}%`;
  return "±0%";
}
