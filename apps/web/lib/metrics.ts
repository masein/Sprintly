// Project flow metrics.

import { api } from "./api";

export type LeadTime = {
  count: number;
  avg_hours: number;
  p50_hours: number;
  p90_hours: number;
};

export type ThroughputPoint = { week_start: string; count: number };

export type Wip = { todo: number; in_progress: number; review: number };

export type Metrics = {
  weeks: number;
  lead_time: LeadTime;
  throughput: ThroughputPoint[];
  wip: Wip;
};

export const getMetrics = (key: string, weeks = 8) =>
  api<Metrics>(
    `/projects/${encodeURIComponent(key)}/metrics?weeks=${weeks}`,
  );

/** Format a duration in hours as "3.5d" / "6h" / "—". */
export function fmtHours(h: number): string {
  if (!h || h <= 0) return "—";
  if (h < 24) return `${h.toFixed(h < 10 ? 1 : 0)}h`;
  return `${(h / 24).toFixed(1)}d`;
}
