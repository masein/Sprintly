"use client";

// /projects/[key]/dashboard — single-pane overview for a project.

import { useQuery } from "@tanstack/react-query";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import {
  AlertTriangle, Calendar, Clock, Flame, History, ListChecks, TrendingUp,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { StatTile } from "@/components/StatTile";
import { VelocityChart } from "@/components/VelocityChart";
import { BurndownChart } from "@/components/BurndownChart";
import { BurnWidget } from "@/components/BurnWidget";
import { getProjectDashboard } from "@/lib/dashboards";
import { getBurndown } from "@/lib/sprints";
import { getProject } from "@/lib/projects";
import { fmtMinutes } from "@/lib/timetracking";
import type { ApiError } from "@/lib/api";

export default function ProjectDashboardPage() {
  const router = useRouter();
  const params = useParams<{ key: string }>();
  const projectKey = params?.key ?? "";

  const q = useQuery({
    queryKey: ["project-dashboard", projectKey],
    queryFn: () => getProjectDashboard(projectKey),
    enabled: !!projectKey,
    refetchInterval: 60_000,
  });
  const projectQ = useQuery({
    queryKey: ["project", projectKey],
    queryFn: () => getProject(projectKey),
    enabled: !!projectKey,
  });
  const canEditBudget = projectQ.data?.your_role === "lead";
  const sprintId = q.data?.current_sprint?.id ?? null;
  const burnQ = useQuery({
    queryKey: ["sprint-burndown", sprintId],
    queryFn: () => getBurndown(sprintId!),
    enabled: !!sprintId,
  });

  if (q.error) {
    const e = q.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    if (e.status === 403 || e.status === 404) {
      return (
        <AppShell currentProjectKey={projectKey}>
          <div className="mono rounded border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-200">
            {e.message}
          </div>
        </AppShell>
      );
    }
  }

  const d = q.data;
  if (!d) {
    return (
      <AppShell currentProjectKey={projectKey}>
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      </AppShell>
    );
  }

  const open = d.status_counts.todo + d.status_counts.in_progress + d.status_counts.review;

  return (
    <AppShell currentProjectKey={projectKey}>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · {projectKey} · dashboard
          </div>
          <h1 className="text-3xl font-semibold">At a glance.</h1>
        </div>
        <div className="flex items-center gap-2">
          <Link
            href={`/projects/${projectKey}`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            ← board
          </Link>
          <Link
            href={`/projects/${projectKey}/sprints`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            sprints →
          </Link>
        </div>
      </header>

      {/* Stat tiles */}
      <section className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatTile
          label="open tasks"
          value={open}
          hint={`${d.status_counts.in_progress} in progress · ${d.status_counts.review} in review`}
        />
        <StatTile
          label="blocked"
          value={d.blocked.count}
          hint={d.blocked.count === 0 ? "none — nice" : "needs unblocking"}
          accent={d.blocked.count > 0 ? "warn" : "good"}
        />
        <StatTile
          label="time this week"
          value={fmtMinutes(d.time_this_week_minutes)}
          hint="across the project"
        />
        <StatTile
          label="done"
          value={d.status_counts.done}
          accent="good"
        />
      </section>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_360px]">
        <div className="space-y-6">
          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <TrendingUp size={11} /> velocity history
            </h2>
            <VelocityChart points={d.velocity_history} />
          </section>

          {d.current_sprint && burnQ.data && (
            <section>
              <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
                <Flame size={11} /> current sprint · {d.current_sprint.name}
              </h2>
              <BurndownChart points={burnQ.data.items} />
            </section>
          )}

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <History size={11} /> recent activity
            </h2>
            <ul className="space-y-1">
              {d.recent_activity.length === 0 && (
                <li className="mono text-[11px] text-chrome-dim">no events yet</li>
              )}
              {d.recent_activity.map((a) => (
                <li key={a.id} className="mono flex items-center gap-2 text-xs">
                  <span className="text-chrome-dim">{relativeTime(a.created_at)}</span>
                  <span className="text-chrome">@{a.actor_handle ?? "?"}</span>
                  <span className="text-chrome-dim">{a.kind}</span>
                  <Link
                    href={`/tasks/${a.task_key}`}
                    className="ml-auto text-accent hover:underline"
                  >
                    {a.task_key}
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        </div>

        <aside className="space-y-6">
          <BurnWidget projectKey={projectKey} canEdit={canEditBudget} />
          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <Calendar size={11} /> upcoming due ({d.upcoming_due.length})
            </h2>
            <ul className="space-y-1">
              {d.upcoming_due.length === 0 && (
                <li className="mono text-[11px] text-chrome-dim">nothing due in 14d</li>
              )}
              {d.upcoming_due.map((t) => (
                <li
                  key={t.task_key}
                  className="rounded border border-white/10 bg-ink-subtle px-2 py-1.5"
                >
                  <div className="flex items-center gap-2">
                    <Link
                      href={`/tasks/${t.task_key}`}
                      className="mono text-xs text-accent hover:underline"
                    >
                      {t.task_key}
                    </Link>
                    <span className="mono ml-auto text-[10px] text-chrome-dim">
                      {t.days_until < 0
                        ? `${-t.days_until}d overdue`
                        : t.days_until === 0
                          ? "today"
                          : `in ${t.days_until}d`}
                    </span>
                  </div>
                  <div className="truncate text-xs text-chrome">{t.title}</div>
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <AlertTriangle size={11} /> blocked samples
            </h2>
            <ul className="space-y-1">
              {d.blocked.samples.length === 0 && (
                <li className="mono text-[11px] text-chrome-dim">none blocked</li>
              )}
              {d.blocked.samples.map((b) => (
                <li
                  key={b.task_key}
                  className="rounded border border-white/10 bg-ink-subtle px-2 py-1.5"
                >
                  <div className="flex items-center gap-2">
                    <Link
                      href={`/tasks/${b.task_key}`}
                      className="mono text-xs text-accent hover:underline"
                    >
                      {b.task_key}
                    </Link>
                    <span className="mono ml-auto text-[10px] text-chrome-dim">
                      blocked by {b.blocked_by_count}
                    </span>
                  </div>
                  <div className="truncate text-xs text-chrome">{b.title}</div>
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <Clock size={11} /> top contributors this week
            </h2>
            <ul className="space-y-1">
              {d.top_contributors.length === 0 && (
                <li className="mono text-[11px] text-chrome-dim">no time logged</li>
              )}
              {d.top_contributors.map((c) => (
                <li
                  key={c.user_id}
                  className="mono flex items-center gap-2 text-xs"
                >
                  <span className="text-chrome">@{c.handle}</span>
                  <span className="ml-auto text-chrome-dim">{fmtMinutes(c.minutes)}</span>
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <ListChecks size={11} /> status breakdown
            </h2>
            <ul className="space-y-1">
              <StatusRow label="to do" n={d.status_counts.todo} />
              <StatusRow label="in progress" n={d.status_counts.in_progress} />
              <StatusRow label="in review" n={d.status_counts.review} />
              <StatusRow label="done" n={d.status_counts.done} />
            </ul>
          </section>
        </aside>
      </div>
    </AppShell>
  );
}

function StatusRow({ label, n }: { label: string; n: number }) {
  return (
    <li className="mono flex items-center text-xs">
      <span className="text-chrome-dim">{label}</span>
      <span className="ml-auto text-chrome">{n}</span>
    </li>
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
