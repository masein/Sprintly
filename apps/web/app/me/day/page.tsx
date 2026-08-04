"use client";

// /me/day — "My day". Personal one-page overview.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import Link from "next/link";
import {
  AlertCircle, Clock, Eye, ListChecks, Timer,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Breadcrumbs } from "@/components/Breadcrumbs";
import { LoadError } from "@/components/LoadError";
import { StatTile } from "@/components/StatTile";
import { WeekNav, thisMondayISO } from "@/components/WeekNav";
import { getMyDashboard } from "@/lib/dashboards";
import { fmtMinutes, specificTimesheet } from "@/lib/timetracking";
import type { ApiError } from "@/lib/api";

const PRIORITY_COLOR: Record<string, string> = {
  p0: "#ef4444",
  p1: "#f59e0b",
  p2: "#a3a3a3",
  p3: "#6b7280",
};

export default function MyDayPage() {
  const router = useRouter();
  const q = useQuery({
    queryKey: ["my-dashboard"],
    queryFn: () => getMyDashboard(),
    // Live-ish by three routes: WS invalidation (see lib/ws.ts) for changes
    // made elsewhere, a focus refetch for coming back to the tab (the global
    // default turns this off), and the interval as the fallback.
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });

  if (q.error) {
    const e = q.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    return (
      <AppShell>
        <LoadError what="Your day" message={e.message} onRetry={() => q.refetch()} />
      </AppShell>
    );
  }
  const d = q.data;
  if (!d) {
    return (
      <AppShell>
        <div className="mono text-sm text-chrome-dim">git fetch --rebase your-stuff…</div>
      </AppShell>
    );
  }

  const open = d.my_status_counts.todo + d.my_status_counts.in_progress + d.my_status_counts.review;

  return (
    <AppShell>
      <header className="mb-6">
        <Breadcrumbs items={[{ label: "sprintly", href: "/" }, { label: "my day" }]} />
        <h1 className="text-3xl font-semibold">Today.</h1>
      </header>

      <section className="mb-6 grid grid-cols-1 gap-3 sm:grid-cols-4">
        <StatTile
          label="assigned to me"
          value={open}
          hint={`${d.my_status_counts.in_progress} in progress`}
        />
        <StatTile
          label="overdue"
          value={d.overdue.length}
          hint={d.overdue.length === 0 ? "nothing past due" : "needs attention"}
          accent={d.overdue.length > 0 ? "warn" : "good"}
        />
        <StatTile
          label="time this week"
          value={fmtMinutes(d.time_this_week_minutes)}
          hint="across all projects"
        />
        <StatTile
          label="watching"
          value={d.watched_changed_recently.length}
          hint="recent changes (7d)"
        />
      </section>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_360px]">
        <div className="space-y-6">
          {d.overdue.length > 0 && (
            <section>
              <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-red-200">
                <AlertCircle size={11} /> overdue ({d.overdue.length})
              </h2>
              <ul className="space-y-1">
                {d.overdue.map((t) => (
                  <li key={t.task_key}>
                    <Link
                      href={`/tasks/${t.task_key}`}
                      className="flex items-center gap-3 rounded border border-red-500/30 bg-red-500/5 px-3 py-2 transition hover:border-red-500/50"
                    >
                      <span className="mono text-xs text-red-200">
                        {-t.days_until}d overdue
                      </span>
                      <span className="mono text-xs text-accent">{t.task_key}</span>
                      <span className="flex-1 truncate text-sm text-chrome">{t.title}</span>
                    </Link>
                  </li>
                ))}
              </ul>
            </section>
          )}

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <ListChecks size={11} /> next up
            </h2>
            <ul className="space-y-1">
              {d.my_tasks_sample.length === 0 && (
                <li className="mono rounded border border-dashed border-white/10 p-4 text-center text-[11px] text-chrome-dim">
                  inbox zero. touch grass.
                </li>
              )}
              {d.my_tasks_sample.map((t) => (
                <li key={t.key}>
                  <Link
                    href={`/tasks/${t.key}`}
                    className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2 transition hover:border-white/20"
                  >
                    <span
                      className="inline-block h-1.5 w-1.5 flex-shrink-0 rounded-full"
                      style={{ background: PRIORITY_COLOR[t.priority] }}
                      aria-hidden
                    />
                    <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
                      {t.status}
                    </span>
                    <span className="mono w-20 flex-shrink-0 text-xs text-chrome-dim">
                      {t.key}
                    </span>
                    <span className="flex-1 truncate text-sm text-chrome">{t.title}</span>
                    <span className="mono text-[10px] text-chrome-dim">
                      {t.project_key}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
            <Link
              href="/me/tasks"
              className="mono mt-2 inline-block text-xs text-accent hover:underline"
            >
              → see all
            </Link>
          </section>
        </div>

        <aside className="space-y-6">
          <ClockworkPanel />

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <Timer size={11} /> running timer
            </h2>
            {d.running_timer ? (
              <Link
                href={`/tasks/${d.running_timer.task_key}`}
                className="block rounded-lg border border-accent/30 bg-accent/10 p-3 transition hover:border-accent/50"
              >
                <div className="mono text-xs text-chrome">
                  {d.running_timer.task_key}
                </div>
                <div className="mono mt-0.5 text-[10px] text-chrome-dim">
                  started {relativeTime(d.running_timer.started_at)} ago
                </div>
              </Link>
            ) : (
              <div className="mono rounded border border-dashed border-white/10 p-3 text-center text-[11px] text-chrome-dim">
                no timer running
              </div>
            )}
          </section>

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <Eye size={11} /> watched · changed
            </h2>
            <ul className="space-y-1">
              {d.watched_changed_recently.length === 0 && (
                <li className="mono text-[11px] text-chrome-dim">nothing changed</li>
              )}
              {d.watched_changed_recently.map((w) => (
                <li
                  key={w.task_key}
                  className="rounded border border-white/10 bg-ink-subtle px-2 py-1.5"
                >
                  <div className="flex items-center gap-2">
                    <Link
                      href={`/tasks/${w.task_key}`}
                      className="mono text-xs text-accent hover:underline"
                    >
                      {w.task_key}
                    </Link>
                    <span className="mono ml-auto text-[10px] text-chrome-dim">
                      {w.last_kind}
                    </span>
                  </div>
                  <div className="truncate text-xs text-chrome">{w.title}</div>
                  <div className="mono text-[10px] text-chrome-dim">
                    {relativeTime(w.last_activity_at)} ago
                  </div>
                </li>
              ))}
            </ul>
          </section>
        </aside>
      </div>
    </AppShell>
  );
}

// ─── Clockwork: my week, any week ───────────────────────────────────────────
// Week-navigable time logs, backed by the per-week timesheet endpoint — the
// answer to "My day only shows the current week". Step back as far as the
// logs go; the timesheets page has the full ledger.

function ClockworkPanel() {
  const [periodStart, setPeriodStart] = useState(() => thisMondayISO());
  const q = useQuery({
    queryKey: ["timesheet", periodStart],
    queryFn: () => specificTimesheet(periodStart),
  });
  const sheet = q.data;
  const maxDay = Math.max(1, ...(sheet?.days.map((d) => d.total_minutes) ?? []));

  return (
    <section aria-label="clockwork">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
          <Clock size={11} /> clockwork
        </h2>
        <WeekNav periodStart={periodStart} onChange={setPeriodStart} />
      </div>

      {q.error ? (
        <div className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-[11px] text-red-200">
          {(q.error as unknown as ApiError).message}
        </div>
      ) : !sheet ? (
        <div className="mono rounded border border-dashed border-white/10 p-3 text-center text-[11px] text-chrome-dim">
          crunching the week…
        </div>
      ) : (
        <div className="rounded-lg border border-white/10 bg-ink-subtle p-3">
          <div className="mono mb-2 flex items-baseline justify-between text-xs">
            <span className="text-chrome-dim">logged</span>
            <span className="text-chrome">{fmtMinutes(sheet.total_minutes)}</span>
          </div>
          <div className="mb-3 flex items-end gap-1" aria-hidden>
            {sheet.days.map((day) => (
              <div key={day.date} className="flex-1">
                <div
                  className="rounded-sm bg-accent/60"
                  style={{ height: `${4 + (day.total_minutes / maxDay) * 28}px` }}
                  title={`${day.date} · ${fmtMinutes(day.total_minutes)}`}
                />
              </div>
            ))}
          </div>
          <ul className="space-y-1">
            {sheet.by_task.length === 0 && (
              <li className="mono text-[11px] text-chrome-dim">
                nothing logged this week
              </li>
            )}
            {sheet.by_task.slice(0, 6).map((t) => (
              <li key={t.task_key} className="mono flex items-center gap-2 text-xs">
                <Link href={`/tasks/${t.task_key}`} className="text-accent hover:underline">
                  {t.task_key}
                </Link>
                <span className="truncate text-chrome-dim">{t.task_title}</span>
                <span className="ml-auto shrink-0 text-chrome">
                  {fmtMinutes(t.total_minutes)}
                </span>
              </li>
            ))}
          </ul>
          <Link
            href="/me/timesheets"
            className="mono mt-2 inline-block text-[11px] text-accent hover:underline"
          >
            → full timesheet
          </Link>
        </div>
      )}
    </section>
  );
}

function relativeTime(iso: string): string {
  const d = new Date(iso);
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return `${Math.floor(diff)}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
