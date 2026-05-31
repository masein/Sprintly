"use client";

// Weekly timesheet view. 7-day grid by day, breakdown by task, totals,
// submit button, CSV download, prev/next week.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { ChevronLeft, ChevronRight, Download } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import {
  csvExportUrl,
  fmtMinutes,
  fmtMoneyCents,
  specificTimesheet,
  submitTimesheet,
} from "@/lib/timetracking";
import type { ApiError } from "@/lib/api";

function thisMondayISO(): string {
  const d = new Date();
  // Move back to Monday (UTC).
  const day = d.getUTCDay(); // 0 = Sun … 6 = Sat
  const offset = day === 0 ? 6 : day - 1;
  d.setUTCDate(d.getUTCDate() - offset);
  return d.toISOString().slice(0, 10);
}

function shiftMondayISO(mondayISO: string, weeks: number): string {
  const d = new Date(`${mondayISO}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + weeks * 7);
  return d.toISOString().slice(0, 10);
}

const STATUS_COPY: Record<string, string> = {
  open: "open · not yet submitted",
  submitted: "submitted · waiting on a lead/admin",
  approved: "approved · locked",
  paid: "paid · locked",
};

export default function TimesheetsPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const [periodStart, setPeriodStart] = useState(() => thisMondayISO());

  const q = useQuery({
    queryKey: ["timesheet", periodStart],
    queryFn: () => specificTimesheet(periodStart),
    retry: (n, e) => (e as ApiError)?.status !== 401 && n < 1,
  });

  const submit = useMutation({
    mutationFn: () => submitTimesheet(periodStart),
    onSuccess: () =>
      qc.invalidateQueries({ queryKey: ["timesheet", periodStart] }),
    onError: (e) => alert((e as ApiError).message),
  });

  if (q.error) {
    const e = q.error as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
  }

  const view = q.data;
  const userId = view?.user_id;

  return (
    <AppShell>
      <header className="mb-6 flex items-end justify-between gap-3">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · timesheet
          </div>
          <h1 className="text-3xl font-semibold">
            Week of {periodStart}
          </h1>
          {view && (
            <div className="mono mt-1 text-xs text-chrome-dim">
              {STATUS_COPY[view.status] ?? view.status}
            </div>
          )}
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={() => setPeriodStart(shiftMondayISO(periodStart, -1))}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <ChevronLeft size={12} /> prev
          </button>
          <button
            type="button"
            onClick={() => setPeriodStart(thisMondayISO())}
            className="mono rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            this week
          </button>
          <button
            type="button"
            onClick={() => setPeriodStart(shiftMondayISO(periodStart, 1))}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            next <ChevronRight size={12} />
          </button>
        </div>
      </header>

      {q.isLoading && (
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      )}

      {view && view.total_minutes === 0 && (
        <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
          <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
            empty week
          </div>
          <p className="text-chrome-dim">
            Either you didn't work this week or you forgot the timer. Both are valid.
          </p>
          <Link
            href="/me/tasks"
            className="mono mt-3 inline-block text-xs text-accent hover:underline"
          >
            → go log some time
          </Link>
        </div>
      )}

      {view && view.total_minutes > 0 && (
        <div className="space-y-6">
          {/* Totals card */}
          <section className="grid grid-cols-2 gap-4 rounded-lg border border-white/10 bg-ink-subtle p-4 sm:grid-cols-4">
            <Stat label="total" value={fmtMinutes(view.total_minutes)} />
            <Stat label="billable" value={fmtMinutes(view.billable_minutes)} />
            <Stat
              label="pay"
              value={fmtMoneyCents(view.total_pay_cents, view.currency)}
            />
            <Stat label="status" value={view.status} mono />
          </section>

          {/* Days */}
          <section>
            <h2 className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
              by day
            </h2>
            <div className="grid grid-cols-7 gap-2">
              {view.days.map((d) => (
                <div
                  key={d.date}
                  className="rounded border border-white/10 bg-ink-subtle p-2"
                >
                  <div className="mono text-[10px] text-chrome-dim">
                    {dayLabel(d.date)}
                  </div>
                  <div className="mono mt-1 text-sm text-chrome">
                    {d.total_minutes === 0 ? "—" : fmtMinutes(d.total_minutes)}
                  </div>
                  {d.billable_minutes > 0 &&
                    d.billable_minutes < d.total_minutes && (
                      <div className="mono text-[9px] text-chrome-dim">
                        {fmtMinutes(d.billable_minutes)} bill.
                      </div>
                    )}
                </div>
              ))}
            </div>
          </section>

          {/* By task */}
          <section>
            <h2 className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
              by task
            </h2>
            <ul className="space-y-1">
              {view.by_task.map((t) => (
                <li
                  key={`${t.project_key}:${t.task_key}`}
                  className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2"
                >
                  <Link
                    href={`/tasks/${t.task_key}`}
                    className="mono text-xs text-accent hover:underline"
                  >
                    {t.task_key}
                  </Link>
                  <span className="flex-1 truncate text-sm text-chrome">
                    {t.task_title}
                  </span>
                  <span className="mono text-xs text-chrome">
                    {fmtMinutes(t.total_minutes)}
                  </span>
                  {t.billable_minutes < t.total_minutes && (
                    <span className="mono text-[10px] text-chrome-dim">
                      {fmtMinutes(t.billable_minutes)} bill.
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </section>

          {/* Actions */}
          <section className="flex items-center justify-end gap-3 border-t border-white/10 pt-4">
            {userId && (
              <a
                href={csvExportUrl(userId, view.period_start)}
                target="_blank"
                rel="noreferrer"
                className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
              >
                <Download size={12} /> csv
              </a>
            )}
            {view.status === "open" && (
              <button
                type="button"
                onClick={() => submit.mutate()}
                disabled={submit.isPending}
                className="mono rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
              >
                {submit.isPending ? "submitting…" : "$ git push timesheet"}
              </button>
            )}
            {view.status !== "open" && (
              <span className="mono text-xs text-chrome-dim">
                {view.status} — no further action
              </span>
            )}
          </section>
        </div>
      )}
    </AppShell>
  );
}

function Stat({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <div className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
        {label}
      </div>
      <div className={`mt-1 text-lg text-chrome ${mono ? "mono" : ""}`}>
        {value}
      </div>
    </div>
  );
}

function dayLabel(iso: string): string {
  const d = new Date(`${iso}T00:00:00Z`);
  const days = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
  return `${days[d.getUTCDay()]} ${iso.slice(5)}`;
}
