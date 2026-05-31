// Payroll API surface.

import { api } from "./api";

export type UserMonthSummary = {
  user_id: string;
  handle: string;
  display_name: string;
  total_minutes: number;
  billable_minutes: number;
  total_pay_cents: number;
  currency: string;
  status: "open" | "paid";
  paid_at: string | null;
};

export type MonthOverview = {
  year: number;
  month: number;
  users: UserMonthSummary[];
  grand_total_pay_cents: number;
  currency: string;
};

export type ProjectLine = {
  project_key: string;
  project_name: string;
  total_minutes: number;
  billable_minutes: number;
};

export type UserMonthDetail = {
  user_id: string;
  handle: string;
  display_name: string;
  year: number;
  month: number;
  total_minutes: number;
  billable_minutes: number;
  total_pay_cents: number;
  currency: string;
  status: "open" | "paid";
  paid_at: string | null;
  by_project: ProjectLine[];
};

export type BurnDto = {
  spent_cents: number;
  budget_cents: number | null;
  currency: string;
  elapsed_fraction: number;
  status: "none" | "ok" | "warn" | "over";
};

export const monthOverview = (year: number, month: number) =>
  api<MonthOverview>(`/payroll/${year}/${pad2(month)}`);

export const userMonth = (userId: string, year: number, month: number) =>
  api<UserMonthDetail>(`/payroll/${userId}/${year}/${pad2(month)}`);

export const csvUrl = (year: number, month: number) => {
  const base =
    process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";
  return `${base}/payroll/${year}/${pad2(month)}.csv`;
};

export const pdfUrl = (userId: string, year: number, month: number) => {
  const base =
    process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";
  return `${base}/payroll/${userId}/${year}/${pad2(month)}.pdf`;
};

export const markPaid = (userId: string, year: number, month: number) =>
  api<void>(`/payroll/${userId}/${year}/${pad2(month)}/mark-paid`, {
    method: "POST",
  });

export const reopen = (userId: string, year: number, month: number) =>
  api<void>(`/payroll/${userId}/${year}/${pad2(month)}/reopen`, {
    method: "POST",
  });

export const setProjectBudget = (
  projectKey: string,
  body: { budget_cents: number | null; budget_currency?: string },
) =>
  api<void>(`/projects/${encodeURIComponent(projectKey)}/budget`, {
    method: "PATCH",
    body,
  });

export const getBurn = (projectKey: string) =>
  api<BurnDto>(`/projects/${encodeURIComponent(projectKey)}/burn`);

function pad2(n: number) {
  return n.toString().padStart(2, "0");
}
