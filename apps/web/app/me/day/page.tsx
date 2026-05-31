"use client";

// /me/day — "My day". Personal one-page overview.

import { useQuery } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import Link from "next/link";
import {
  AlertCircle, Eye, ListChecks, Timer,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { StatTile } from "@/components/StatTile";
import { getMyDashboard } from "@/lib/dashboards";
import { fmtMinutes } from "@/lib/timetracking";
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
    refetchInterval: 60_000,
  });

  if (q.error) {
    const e = q.error as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
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
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · my day
        </div>
        <h1 className="text-3xl font-semibold">Today.</h1>
      </header>

      <section className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
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

function relativeTime(iso: string): string {
  const d = new Date(iso);
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return `${Math.floor(diff)}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
