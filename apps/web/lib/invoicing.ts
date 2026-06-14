// Per-client billing (F14). Admin-only.

import { api } from "./api";

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

export type Client = {
  id: string;
  name: string;
  email: string | null;
  address: string | null;
  currency: string;
  notes: string;
  created_at: string;
};

export type InvoiceStatus = "draft" | "sent" | "paid";

export type Invoice = {
  id: string;
  client_id: string;
  number: string;
  status: InvoiceStatus;
  period_start: string;
  period_end: string;
  currency: string;
  subtotal_cents: number;
  total_cents: number;
  notes: string;
  issued_at: string;
  sent_at: string | null;
  paid_at: string | null;
  created_at: string;
};

export type InvoiceLine = {
  id: string;
  project_id: string | null;
  description: string;
  minutes: number;
  rate_cents: number;
  amount_cents: number;
  sort: number;
};

export type InvoiceWithLines = Invoice & {
  client_name: string;
  lines: InvoiceLine[];
};

// ── clients ──
export const listClients = () => api<Client[]>("/clients");
export const createClient = (body: {
  name: string;
  email?: string;
  address?: string;
  currency?: string;
  notes?: string;
}) => api<Client>("/clients", { method: "POST", body });
export const deleteClient = (id: string) =>
  api<void>(`/clients/${id}`, { method: "DELETE" });
export const setProjectClient = (projectKey: string, client_id: string | null) =>
  api<void>(`/projects/${encodeURIComponent(projectKey)}/client`, {
    method: "PUT",
    body: { client_id },
  });

// ── invoices ──
export const listInvoices = (clientId?: string) =>
  api<{ items: Invoice[] }>(
    `/invoices${clientId ? `?client_id=${clientId}` : ""}`,
  ).then((r) => r.items);
export const getInvoice = (id: string) => api<InvoiceWithLines>(`/invoices/${id}`);
export const generateInvoice = (body: {
  client_id: string;
  period_start: string;
  period_end: string;
}) => api<InvoiceWithLines>("/invoices", { method: "POST", body });
export const markInvoiceSent = (id: string) =>
  api<void>(`/invoices/${id}/mark-sent`, { method: "POST" });
export const markInvoicePaid = (id: string) =>
  api<void>(`/invoices/${id}/mark-paid`, { method: "POST" });
export const deleteInvoice = (id: string) =>
  api<void>(`/invoices/${id}`, { method: "DELETE" });

export const invoicePdfUrl = (id: string) => `${API_BASE}/invoices/${id}/pdf`;
export const invoiceCsvUrl = (id: string) => `${API_BASE}/invoices/${id}/csv`;
