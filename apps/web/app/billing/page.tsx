"use client";

// /billing — admin-only per-client invoicing (F14): manage clients, link
// projects to a client, generate an invoice from billable time over a period,
// then export (PDF/CSV) and walk it through draft → sent → paid.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { Check, Download, FileText, Receipt, Send, Trash2 } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { fmtMoneyCents } from "@/lib/timetracking";
import {
  createClient,
  deleteClient,
  deleteInvoice,
  generateInvoice,
  invoiceCsvUrl,
  invoicePdfUrl,
  listClients,
  listInvoices,
  markInvoicePaid,
  markInvoiceSent,
  setProjectClient,
  type InvoiceStatus,
} from "@/lib/invoicing";
import type { ApiError } from "@/lib/api";

const noRetryOnAuth = (n: number, e: unknown) =>
  (e as ApiError)?.status !== 401 && (e as ApiError)?.status !== 403 && n < 1;

export default function BillingPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);

  const clientsQ = useQuery({ queryKey: ["clients"], queryFn: listClients, retry: noRetryOnAuth });
  const invoicesQ = useQuery({ queryKey: ["invoices"], queryFn: () => listInvoices(), retry: noRetryOnAuth });

  const forbidden =
    (clientsQ.error as ApiError)?.status === 403 ||
    (invoicesQ.error as ApiError)?.status === 403;
  const unauthed =
    (clientsQ.error as ApiError)?.status === 401 ||
    (invoicesQ.error as ApiError)?.status === 401;
  if (unauthed) {
    router.push("/login");
  }

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ["clients"] });
    qc.invalidateQueries({ queryKey: ["invoices"] });
  };
  const fail = (e: unknown) => setError((e as ApiError).message ?? "failed");

  // ── client create form ──
  const [cName, setCName] = useState("");
  const [cEmail, setCEmail] = useState("");
  const [cCurrency, setCCurrency] = useState("USD");
  const addClient = useMutation({
    mutationFn: () =>
      createClient({ name: cName.trim(), email: cEmail.trim() || undefined, currency: cCurrency.trim() || undefined }),
    onSuccess: () => { setCName(""); setCEmail(""); setError(null); invalidate(); },
    onError: fail,
  });
  const rmClient = useMutation({ mutationFn: (id: string) => deleteClient(id), onSuccess: invalidate, onError: fail });

  // ── link project ──
  const [linkKey, setLinkKey] = useState("");
  const [linkClient, setLinkClient] = useState("");
  const link = useMutation({
    mutationFn: () => setProjectClient(linkKey.trim().toUpperCase(), linkClient || null),
    onSuccess: () => { setLinkKey(""); setError(null); },
    onError: fail,
  });

  // ── generate invoice ──
  const [genClient, setGenClient] = useState("");
  const [genStart, setGenStart] = useState("");
  const [genEnd, setGenEnd] = useState("");
  const gen = useMutation({
    mutationFn: () => generateInvoice({ client_id: genClient, period_start: genStart, period_end: genEnd }),
    onSuccess: () => { setError(null); invalidate(); },
    onError: fail,
  });

  const sent = useMutation({ mutationFn: (id: string) => markInvoiceSent(id), onSuccess: invalidate, onError: fail });
  const paid = useMutation({ mutationFn: (id: string) => markInvoicePaid(id), onSuccess: invalidate, onError: fail });
  const rmInvoice = useMutation({ mutationFn: (id: string) => deleteInvoice(id), onSuccess: invalidate, onError: fail });

  const clients = clientsQ.data ?? [];
  const invoices = invoicesQ.data ?? [];
  const clientName = (id: string) => clients.find((c) => c.id === id)?.name ?? "—";

  if (forbidden) {
    return (
      <AppShell>
        <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
          Billing is admin-only. Ask an admin for access.
        </div>
      </AppShell>
    );
  }

  return (
    <AppShell>
      <header className="mb-6">
        <div className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
          <Receipt size={12} /> sprintly · billing
        </div>
        <h1 className="text-3xl font-semibold">Invoices.</h1>
        <p className="mt-1 text-sm text-chrome-dim">
          Roll up billable time on a client&apos;s projects into an invoice, at each
          contributor&apos;s configured rate.
        </p>
      </header>

      {error && (
        <div className="mono mb-4 rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200">
          {error}
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-2">
        {/* Clients */}
        <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
          <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">clients</h2>
          <ul className="space-y-1">
            {clients.map((c) => (
              <li key={c.id} className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5">
                <span className="text-sm text-chrome">{c.name}</span>
                <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-chrome-dim">{c.currency}</span>
                {c.email && <span className="mono truncate text-[11px] text-chrome-dim">{c.email}</span>}
                <button
                  type="button"
                  aria-label={`delete ${c.name}`}
                  onClick={() => { if (confirm(`Delete client "${c.name}"? Its invoices stay.`)) rmClient.mutate(c.id); }}
                  className="ml-auto text-chrome-dim hover:text-red-300"
                >
                  <Trash2 size={13} />
                </button>
              </li>
            ))}
            {clients.length === 0 && (
              <li className="mono text-[11px] text-chrome-dim">no clients yet — add one to start billing</li>
            )}
          </ul>
          <form
            onSubmit={(e) => { e.preventDefault(); if (cName.trim()) addClient.mutate(); }}
            className="flex flex-wrap items-center gap-2 border-t border-white/10 pt-3"
          >
            <input value={cName} onChange={(e) => setCName(e.target.value)} placeholder="client name" aria-label="client name"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none" />
            <input value={cEmail} onChange={(e) => setCEmail(e.target.value)} placeholder="email (optional)" aria-label="client email"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none" />
            <input value={cCurrency} onChange={(e) => setCCurrency(e.target.value)} maxLength={3} aria-label="currency"
              className="mono w-16 rounded border border-white/10 bg-ink px-2 py-1 text-xs uppercase text-chrome focus:border-accent focus:outline-none" />
            <button type="submit" disabled={addClient.isPending || !cName.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50">add</button>
          </form>

          <form
            onSubmit={(e) => { e.preventDefault(); if (linkKey.trim() && linkClient) link.mutate(); }}
            className="flex flex-wrap items-center gap-2 border-t border-white/10 pt-3"
          >
            <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">link project</span>
            <input value={linkKey} onChange={(e) => setLinkKey(e.target.value)} placeholder="PROJECT KEY" aria-label="project key"
              className="mono w-28 rounded border border-white/10 bg-ink px-2 py-1 text-xs uppercase text-chrome focus:border-accent focus:outline-none" />
            <select value={linkClient} onChange={(e) => setLinkClient(e.target.value)} aria-label="link to client"
              className="mono flex-1 rounded border border-white/10 bg-ink px-1.5 py-1 text-xs text-chrome">
              <option value="">to client…</option>
              {clients.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
            <button type="submit" disabled={link.isPending || !linkKey.trim() || !linkClient}
              className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:text-chrome disabled:opacity-50">link</button>
          </form>
        </section>

        {/* Generate */}
        <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
          <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">generate invoice</h2>
          <form onSubmit={(e) => { e.preventDefault(); if (genClient && genStart && genEnd) gen.mutate(); }} className="space-y-2">
            <select value={genClient} onChange={(e) => setGenClient(e.target.value)} aria-label="client"
              className="mono w-full rounded border border-white/10 bg-ink px-2 py-1.5 text-xs text-chrome">
              <option value="">pick a client…</option>
              {clients.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
            <div className="flex items-center gap-2">
              <label className="mono flex-1 text-[11px] text-chrome-dim">
                from
                <input type="date" value={genStart} onChange={(e) => setGenStart(e.target.value)} aria-label="period start"
                  className="mono mt-1 w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none" />
              </label>
              <label className="mono flex-1 text-[11px] text-chrome-dim">
                to
                <input type="date" value={genEnd} onChange={(e) => setGenEnd(e.target.value)} aria-label="period end"
                  className="mono mt-1 w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none" />
              </label>
            </div>
            <button type="submit" disabled={gen.isPending || !genClient || !genStart || !genEnd}
              className="mono w-full rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50">
              {gen.isPending ? "rolling up…" : "$ invoice --generate"}
            </button>
          </form>
          <p className="mono text-[10px] text-chrome-dim">
            Pulls billable, completed time logs on the client&apos;s projects in the
            period. Empty periods are rejected — nothing to bill.
          </p>
        </section>
      </div>

      {/* Invoices */}
      <section className="mt-6 space-y-2">
        <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">invoices</h2>
        <ul className="space-y-1">
          {invoices.map((inv) => (
            <li key={inv.id} className="flex flex-wrap items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2">
              <StatusBadge status={inv.status} />
              <span className="mono text-sm text-chrome">{inv.number}</span>
              <span className="mono text-xs text-chrome-dim">{clientName(inv.client_id)}</span>
              <span className="mono text-[11px] text-chrome-dim">{inv.period_start} → {inv.period_end}</span>
              <span className="mono ml-auto text-sm text-chrome">{fmtMoneyCents(inv.total_cents, inv.currency)}</span>
              <a href={invoicePdfUrl(inv.id)} title="PDF" className="text-chrome-dim hover:text-chrome"><FileText size={14} /></a>
              <a href={invoiceCsvUrl(inv.id)} title="CSV" className="text-chrome-dim hover:text-chrome"><Download size={14} /></a>
              {inv.status === "draft" && (
                <button type="button" title="mark sent" onClick={() => sent.mutate(inv.id)} className="text-chrome-dim hover:text-accent"><Send size={14} /></button>
              )}
              {inv.status !== "paid" && (
                <button type="button" title="mark paid" onClick={() => paid.mutate(inv.id)} className="text-chrome-dim hover:text-emerald-300"><Check size={15} /></button>
              )}
              {inv.status === "draft" && (
                <button type="button" aria-label={`delete ${inv.number}`} onClick={() => { if (confirm(`Delete draft ${inv.number}?`)) rmInvoice.mutate(inv.id); }} className="text-chrome-dim hover:text-red-300"><Trash2 size={13} /></button>
              )}
            </li>
          ))}
          {invoices.length === 0 && (
            <li className="mono rounded border border-dashed border-white/10 p-4 text-center text-xs text-chrome-dim">
              no invoices yet — generate one above
            </li>
          )}
        </ul>
      </section>
    </AppShell>
  );
}

function StatusBadge({ status }: { status: InvoiceStatus }) {
  const cls =
    status === "paid"
      ? "border-emerald-500/30 text-emerald-300"
      : status === "sent"
        ? "border-accent/40 text-accent"
        : "border-white/10 text-chrome-dim";
  return (
    <span className={`mono inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${cls}`}>
      {status}
    </span>
  );
}
