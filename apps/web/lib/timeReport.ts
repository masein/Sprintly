// Flexible time report: logged time over a custom range and/or a sprint,
// grouped by user / task / sprint, with totals + billable + pay + CSV.

import { api } from "./api";

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

export type TimeUserRow = {
  user_id: string;
  handle: string;
  display_name: string;
  total_minutes: number;
  billable_minutes: number;
  pay_cents: number;
  currency: string;
};

export type TimeTaskRow = {
  task_key: string;
  task_title: string;
  total_minutes: number;
  billable_minutes: number;
};

export type TimeSprintRow = {
  sprint_id: string | null;
  sprint_name: string | null;
  total_minutes: number;
  billable_minutes: number;
};

export type TimeReport = {
  project_key: string;
  project_name: string;
  from: string | null;
  to: string | null;
  sprint_id: string | null;
  sprint_name: string | null;
  scope: "team" | "self";
  total_minutes: number;
  billable_minutes: number;
  total_pay_cents: number;
  currency: string;
  by_user: TimeUserRow[];
  by_task: TimeTaskRow[];
  by_sprint: TimeSprintRow[];
};

export type TimeReportParams = { from?: string; to?: string; sprintId?: string };

function qs(p: TimeReportParams): string {
  const s = new URLSearchParams();
  if (p.from) s.set("from", p.from);
  if (p.to) s.set("to", p.to);
  if (p.sprintId) s.set("sprint_id", p.sprintId);
  return s.toString();
}

export const getTimeReport = (key: string, p: TimeReportParams = {}) =>
  api<TimeReport>(`/projects/${encodeURIComponent(key)}/time-report?${qs(p)}`);

/** Same query as the report, plus `format=csv` — a plain browser download. */
export function timeReportCsvUrl(key: string, p: TimeReportParams = {}): string {
  const s = qs(p);
  return `${API_BASE}/projects/${encodeURIComponent(key)}/time-report?${
    s ? `${s}&` : ""
  }format=csv`;
}

/** Minutes → "3h 20m" / "45m" / "—". */
export function fmtMinutes(m: number): string {
  if (!m || m <= 0) return "—";
  const h = Math.floor(m / 60);
  const mm = m % 60;
  if (h === 0) return `${mm}m`;
  if (mm === 0) return `${h}h`;
  return `${h}h ${mm}m`;
}

/** Cents → localized currency; "MIXED" is passed through as a plain amount. */
export function fmtMoney(cents: number, currency: string): string {
  const amount = cents / 100;
  if (currency === "MIXED") return `${amount.toFixed(2)} (mixed)`;
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency,
    }).format(amount);
  } catch {
    return `${amount.toFixed(2)} ${currency}`;
  }
}
