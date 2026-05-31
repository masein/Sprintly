"use client";

// /payroll — admin-only monthly summary across all users.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { Check, ChevronLeft, ChevronRight, Download, FileText, RotateCcw } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import {
  csvUrl,
  markPaid,
  monthOverview,
  pdfUrl,
  reopen,
} from "@/lib/payroll";
import { fmtMinutes, fmtMoneyCents } from "@/lib/timetracking";
import type { ApiError } from "@/lib/api";

export default function PayrollPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getUTCFullYear());
  const [month, setMonth] = useState(now.getUTCMonth() + 1);

  const q = useQuery({
    queryKey: ["payroll", year, month],
    queryFn: () => monthOverview(year, month),
    retry: (n, e) => (e as ApiError)?.status !== 401 && (e as ApiError)?.status !== 403 && n < 1,
  });

  if (q.error) {
    const e = q.error as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    if (e.status === 403) {
      return (
        <AppShell>
          <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
            Payroll is admin-only.
          </div>
        </AppShell>
      );
    }
  }

  const paid = useMutation({
    mutationFn: ({ user_id }: { user_id: string }) => markPaid(user_id, year, month),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["payroll", year, month] }),
  });
  const undo = useMutation({
    mutationFn: ({ user_id }: { user_id: string }) => reopen(user_id, year, month),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["payroll", year, month] }),
  });

  function shift(delta: number) {
    const m0 = year * 12 + (month - 1) + delta;
    setYear(Math.floor(m0 / 12));
    setMonth((m0 % 12) + 1);
  }

  const data = q.data;
  return (
    <AppShell>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · payroll
          </div>
          <h1 className="text-3xl font-semibold">
            {monthLabel(year, month)}
          </h1>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={() => shift(-1)}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <ChevronLeft size={12} /> prev
          </button>
          <button
            type="button"
            onClick={() => {
              setYear(now.getUTCFullYear());
              setMonth(now.getUTCMonth() + 1);
            }}
            className="mono rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            this month
          </button>
          <button
            type="button"
            onClick={() => shift(1)}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            next <ChevronRight size={12} />
          </button>
          {data && (
            <a
              href={csvUrl(year, month)}
              target="_blank"
              rel="noreferrer"
              className="mono ml-2 inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <Download size={12} /> csv
            </a>
          )}
        </div>
      </header>

      {q.isLoading && (
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      )}

      {data && (
        <>
          <section className="mb-6 rounded-lg border border-white/10 bg-ink-subtle p-4">
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              grand total
            </div>
            <div className="text-2xl text-chrome">
              {fmtMoneyCents(data.grand_total_pay_cents, data.currency)}
            </div>
          </section>

          <ul className="space-y-2">
            {data.users.map((u) => (
              <li
                key={u.user_id}
                className="flex items-center gap-3 rounded-lg border border-white/10 bg-ink-subtle px-4 py-3"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="mono text-sm text-chrome">@{u.handle}</span>
                    <span className="mono text-xs text-chrome-dim">· {u.display_name}</span>
                    <span
                      className={`mono ml-2 rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${
                        u.status === "paid"
                          ? "border-emerald-500/30 text-emerald-300"
                          : "border-white/10 text-chrome-dim"
                      }`}
                    >
                      {u.status}
                    </span>
                  </div>
                  <div className="mono text-xs text-chrome-dim">
                    {fmtMinutes(u.total_minutes)} · {fmtMinutes(u.billable_minutes)} billable
                  </div>
                </div>
                <div className="mono text-right">
                  <div className="text-sm text-chrome">
                    {fmtMoneyCents(u.total_pay_cents, u.currency)}
                  </div>
                  {u.paid_at && (
                    <div className="text-[10px] text-chrome-dim">
                      paid {u.paid_at.slice(0, 10)}
                    </div>
                  )}
                </div>
                <a
                  href={pdfUrl(u.user_id, year, month)}
                  target="_blank"
                  rel="noreferrer"
                  className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
                >
                  <FileText size={11} /> pdf
                </a>
                {u.status === "open" ? (
                  <button
                    type="button"
                    onClick={() => paid.mutate({ user_id: u.user_id })}
                    disabled={paid.isPending}
                    className="mono inline-flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-xs text-accent-fg hover:opacity-90 disabled:opacity-50"
                  >
                    <Check size={11} /> mark paid
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => undo.mutate({ user_id: u.user_id })}
                    disabled={undo.isPending}
                    className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome disabled:opacity-50"
                    title="reopen"
                  >
                    <RotateCcw size={11} /> reopen
                  </button>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
    </AppShell>
  );
}

function monthLabel(year: number, month: number): string {
  const months = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
  ];
  return `${months[month - 1]} ${year}`;
}
