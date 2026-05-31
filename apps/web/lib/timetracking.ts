// Timer + timesheet API surface.

import { api } from "./api";

export type TimeLog = {
  id: string;
  task_id: string;
  task_key: string;
  project_key: string;
  user_id: string;
  started_at: string;
  ended_at: string | null;
  duration_minutes: number | null;
  note: string;
  billable: boolean;
};

export const startTimer = (taskKey: string) =>
  api<TimeLog>(`/tasks/${encodeURIComponent(taskKey)}/timer/start`, { method: "POST" });

export const stopTimer = () =>
  api<{ stopped: TimeLog | null }>("/timer/stop", { method: "POST" });

export const currentTimer = () =>
  api<{ running: TimeLog | null }>("/me/timer");

export const createManualLog = (
  taskKey: string,
  body: { started_at: string; duration_minutes: number; note?: string; billable?: boolean },
) =>
  api<TimeLog>(`/tasks/${encodeURIComponent(taskKey)}/time-logs`, {
    method: "POST",
    body,
  });

export const listTaskLogs = (taskKey: string) =>
  api<{ items: TimeLog[] }>(`/tasks/${encodeURIComponent(taskKey)}/time-logs`)
    .then((r) => r.items);

export const editLog = (
  id: string,
  body: { note?: string; billable?: boolean; duration_minutes?: number },
) => api<TimeLog>(`/time-logs/${id}`, { method: "PATCH", body });

export const deleteLog = (id: string) =>
  api<void>(`/time-logs/${id}`, { method: "DELETE" });

// ── Timesheets ──────────────────────────────────────────────────────────────

export type TimesheetView = {
  user_id: string;
  period_start: string;
  period_end: string;
  status: "open" | "submitted" | "approved" | "paid";
  total_minutes: number;
  billable_minutes: number;
  total_pay_cents: number;
  currency: string;
  days: { date: string; total_minutes: number; billable_minutes: number }[];
  by_task: {
    task_key: string;
    project_key: string;
    task_title: string;
    total_minutes: number;
    billable_minutes: number;
  }[];
};

export const currentTimesheet = () =>
  api<TimesheetView>("/me/timesheets/current");

export const specificTimesheet = (periodStart: string) =>
  api<TimesheetView>(`/me/timesheets/${periodStart}`);

export const submitTimesheet = (periodStart: string) =>
  api<void>(`/me/timesheets/${periodStart}/submit`, { method: "POST" });

export const approveTimesheet = (userId: string, periodStart: string) =>
  api<void>(`/timesheets/${userId}/${periodStart}/approve`, { method: "POST" });

export const markTimesheetPaid = (userId: string, periodStart: string) =>
  api<void>(`/timesheets/${userId}/${periodStart}/mark-paid`, { method: "POST" });

export const csvExportUrl = (userId: string, periodStart: string) => {
  const base = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";
  return `${base}/timesheets/${userId}/${periodStart}.csv`;
};

export type PendingApproval = {
  user_id: string;
  handle: string;
  display_name: string;
  period_start: string;
  period_end: string;
  total_minutes: number;
  billable_minutes: number;
  total_pay_cents: number;
  currency: string;
};

export const pendingApprovals = () =>
  api<{ items: PendingApproval[] }>("/timesheets/pending").then((r) => r.items);

// ── helpers ─────────────────────────────────────────────────────────────────

export function fmtMinutes(min: number): string {
  if (min < 60) return `${min}m`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

export function fmtMoneyCents(cents: number, currency: string): string {
  const amount = (cents / 100).toFixed(2);
  return `${currency} ${amount}`;
}
