"use client";

// The Time report: logged time over a custom range and/or a sprint, grouped by
// user / task / sprint, with totals + billable + pay + CSV. Lives as the "Time"
// tab on a project's metrics page. Permissions are enforced server-side (leads
// see the team, members see themselves) — the UI just labels the scope.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Download } from "lucide-react";
import {
  fmtMinutes,
  fmtMoney,
  getTimeReport,
  timeReportCsvUrl,
  type TimeReportParams,
} from "@/lib/timeReport";
import { listSprints } from "@/lib/sprints";
import type { ApiError } from "@/lib/api";

function ymd(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}
function daysAgo(n: number): string {
  const d = new Date();
  d.setDate(d.getDate() - n);
  return ymd(d);
}
function monthStart(): string {
  const d = new Date();
  return ymd(new Date(d.getFullYear(), d.getMonth(), 1));
}

const activeIdle = (on: boolean) =>
  `mono rounded border px-2 py-1 text-xs ${
    on
      ? "border-accent text-chrome"
      : "border-white/10 text-chrome-dim hover:border-white/20 hover:text-chrome"
  }`;

export function TimeReportPanel({ projectKey }: { projectKey: string }) {
  const today = ymd(new Date());
  const [from, setFrom] = useState(daysAgo(30));
  const [to, setTo] = useState(today);
  const [sprintId, setSprintId] = useState<string>("");

  const sprintsQ = useQuery({
    queryKey: ["sprints", projectKey],
    queryFn: () => listSprints(projectKey),
    retry: false,
  });
  const sprints = sprintsQ.data ?? [];

  const params: TimeReportParams = useMemo(
    () => ({ from: from || undefined, to: to || undefined, sprintId: sprintId || undefined }),
    [from, to, sprintId],
  );

  const q = useQuery({
    queryKey: ["time-report", projectKey, params],
    queryFn: () => getTimeReport(projectKey, params),
    retry: (n, e) => (e as unknown as ApiError)?.status !== 403 && n < 1,
  });

  function preset(kind: "7d" | "30d" | "month" | "sprint") {
    if (kind === "sprint") {
      const active = sprints.find((s) => s.state === "active") ?? sprints[0];
      if (active) {
        setSprintId(active.id);
        setFrom("");
        setTo("");
      }
      return;
    }
    setSprintId("");
    setTo(today);
    setFrom(kind === "7d" ? daysAgo(7) : kind === "30d" ? daysAgo(30) : monthStart());
  }

  const report = q.data;
  const hasSprint = sprints.length > 0;

  return (
    <div className="space-y-6">
      {/* Controls */}
      <div className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
        <div className="flex flex-wrap items-center gap-2">
          <span className="mono text-xs uppercase tracking-widest text-chrome-dim">range</span>
          <button type="button" className={activeIdle(false)} onClick={() => preset("7d")}>
            last 7d
          </button>
          <button type="button" className={activeIdle(false)} onClick={() => preset("30d")}>
            last 30d
          </button>
          <button type="button" className={activeIdle(false)} onClick={() => preset("month")}>
            this month
          </button>
          <button
            type="button"
            className={activeIdle(false)}
            onClick={() => preset("sprint")}
            disabled={!hasSprint}
            title={hasSprint ? "" : "no sprints yet"}
          >
            this sprint
          </button>
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <label className="mono flex flex-col gap-1 text-[11px] text-chrome-dim">
            from
            <input
              type="date"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              aria-label="from date"
              className="mono rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome"
            />
          </label>
          <label className="mono flex flex-col gap-1 text-[11px] text-chrome-dim">
            to
            <input
              type="date"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              aria-label="to date"
              className="mono rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome"
            />
          </label>
          <label className="mono flex flex-col gap-1 text-[11px] text-chrome-dim">
            sprint
            <select
              value={sprintId}
              onChange={(e) => setSprintId(e.target.value)}
              aria-label="sprint"
              className="mono rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome"
            >
              <option value="">— any —</option>
              {sprints.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                  {s.state === "active" ? " (active)" : ""}
                </option>
              ))}
            </select>
          </label>
          <a
            href={timeReportCsvUrl(projectKey, params)}
            target="_blank"
            rel="noreferrer"
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <Download size={12} /> csv
          </a>
        </div>
      </div>

      {q.isLoading && <div className="mono text-xs text-chrome-dim">compiling vibes…</div>}
      {q.error && (
        <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
          {(q.error as unknown as ApiError)?.status === 403
            ? "you don't have access to this project's time."
            : "couldn't load the time report."}
        </div>
      )}

      {report && (
        <>
          {/* Totals */}
          <div className="grid grid-cols-3 gap-3">
            <Tile label="total" value={fmtMinutes(report.total_minutes)} />
            <Tile
              label="billable"
              value={fmtMinutes(report.billable_minutes)}
              sub={`${pct(report.billable_minutes, report.total_minutes)}%`}
            />
            <Tile label="pay" value={fmtMoney(report.total_pay_cents, report.currency)} />
          </div>
          <div className="mono text-[11px] text-chrome-dim">
            scope: {report.scope === "team" ? "team (all members)" : "you only"}
            {report.sprint_name ? ` · sprint: ${report.sprint_name}` : ""}
            {report.from || report.to
              ? ` · ${report.from ?? "…"} → ${report.to ?? "…"}`
              : ""}
          </div>

          <Group title="by person">
            <Table
              head={["person", "total", "billable", "pay"]}
              rows={report.by_user.map((u) => [
                `@${u.handle}`,
                fmtMinutes(u.total_minutes),
                fmtMinutes(u.billable_minutes),
                fmtMoney(u.pay_cents, u.currency),
              ])}
              empty="no time logged in this range."
            />
          </Group>

          <Group title="by task">
            <Table
              head={["task", "total", "billable"]}
              rows={report.by_task.map((t) => [
                `${t.task_key} · ${t.task_title}`,
                fmtMinutes(t.total_minutes),
                fmtMinutes(t.billable_minutes),
              ])}
              empty="no time logged in this range."
            />
          </Group>

          <Group title="by sprint">
            <Table
              head={["sprint", "total", "billable"]}
              rows={report.by_sprint.map((s) => [
                s.sprint_name ?? "— no sprint —",
                fmtMinutes(s.total_minutes),
                fmtMinutes(s.billable_minutes),
              ])}
              empty="no time logged in this range."
            />
          </Group>
        </>
      )}
    </div>
  );
}

function pct(n: number, d: number): number {
  return d > 0 ? Math.round((n / d) * 100) : 0;
}

function Tile({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mono text-xs uppercase tracking-widest text-chrome-dim">{label}</div>
      <div className="mt-1 text-2xl text-chrome">{value}</div>
      {sub && <div className="mono text-[11px] text-chrome-dim">{sub}</div>}
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-2">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">{title}</h2>
      {children}
    </section>
  );
}

function Table({
  head,
  rows,
  empty,
}: {
  head: string[];
  rows: string[][];
  empty: string;
}) {
  if (rows.length === 0) {
    return <div className="mono text-[11px] text-chrome-dim">{empty}</div>;
  }
  return (
    <div className="overflow-x-auto rounded-lg border border-white/10">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-white/10 text-left">
            {head.map((h) => (
              <th key={h} className="mono px-3 py-2 text-[11px] uppercase tracking-widest text-chrome-dim">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="border-b border-white/5 last:border-0">
              {r.map((cell, j) => (
                <td key={j} className={`px-3 py-2 ${j === 0 ? "text-chrome" : "mono text-chrome-dim"}`}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
