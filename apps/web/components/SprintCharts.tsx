"use client";

// The sprint charts card: Burndown · Burnup · Velocity, with the sprint's KPIs
// alongside — progress, scope change, days, and how the open work is doing.
// One card on the project dashboard and on each sprint page (QA report 6 +
// the KPI request: "charts are empty and need UI improvement", "project-level
// KPIs near the charts").
//
// The series come from `GET /sprints/:id/stats`, which measures in story
// points when the sprint has any and in tasks when nobody estimated — so an
// unestimated sprint still gets a real chart instead of a flat line at zero.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  ComposedChart,
  Legend,
  Line,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { TrendingDown, TrendingUp, BarChart3 } from "lucide-react";
import { getSprintStats, getVelocity, type SprintStats } from "@/lib/sprints";
import {
  VELOCITY_UNITS,
  percent,
  signedPercent,
  summarizeVelocity,
  velocityValue,
  type VelocityUnit,
} from "@/lib/kpi";
import { ProgressRing } from "./ProgressRing";

type Tab = "burndown" | "burnup" | "velocity";

const AXIS = { fontSize: 11, fontFamily: "JetBrains Mono, monospace" };
const TOOLTIP = {
  contentStyle: {
    background: "#111114",
    border: "1px solid #ffffff20",
    borderRadius: 6,
    fontSize: 12,
    fontFamily: "JetBrains Mono, monospace",
  },
  labelStyle: { color: "#e6e6ea" },
};

const fmtDay = (iso: string) =>
  new Date(`${iso}T00:00:00Z`).toLocaleDateString("en", {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });

export function SprintCharts({
  projectKey,
  sprintId,
}: {
  projectKey: string;
  /** The sprint to chart; null when nothing is running (velocity still shows). */
  sprintId: string | null;
}) {
  const [tab, setTab] = useState<Tab>(sprintId ? "burndown" : "velocity");
  const statsQ = useQuery({
    queryKey: ["sprint-stats", sprintId],
    queryFn: () => getSprintStats(sprintId!),
    enabled: !!sprintId,
  });
  const stats = statsQ.data ?? null;

  const title =
    tab === "velocity" ? "Velocity" : tab === "burnup" ? "Sprint burn up" : "Sprint burn down";
  const Icon = tab === "velocity" ? BarChart3 : tab === "burnup" ? TrendingUp : TrendingDown;

  return (
    <section
      aria-label="sprint charts"
      data-sprint-charts
      className="rounded-lg border border-white/10 bg-ink-subtle p-4"
    >
      <header className="mb-3 flex flex-wrap items-start justify-between gap-3">
        <div className="flex items-center gap-3">
          <span className="flex h-9 w-9 items-center justify-center rounded bg-accent/15 text-accent">
            <Icon size={18} />
          </span>
          <div>
            <h2 className="font-semibold leading-tight">{title}</h2>
            <div className="mono text-[11px] text-chrome-dim">
              {stats
                ? `${stats.sprint.name} · ${fmtDay(stats.sprint.starts_at.slice(0, 10))} – ${fmtDay(
                    stats.sprint.ends_at.slice(0, 10),
                  )}`
                : sprintId
                  ? "nudging electrons…"
                  : "no sprint running"}
            </div>
          </div>
        </div>
        <div role="tablist" aria-label="chart" className="flex rounded border border-white/10 p-0.5">
          {(["burndown", "burnup", "velocity"] as const).map((t) => (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={`mono rounded px-2.5 py-1 text-xs capitalize transition ${
                tab === t ? "bg-accent/20 text-chrome" : "text-chrome-dim hover:text-chrome"
              }`}
            >
              {t}
            </button>
          ))}
        </div>
      </header>

      {tab === "velocity" ? (
        <VelocityPanel projectKey={projectKey} />
      ) : !sprintId ? (
        <Empty>No sprint is running. Start one and its burndown draws itself from day one.</Empty>
      ) : statsQ.error ? (
        <Empty>Couldn&apos;t load the sprint&apos;s numbers — {String((statsQ.error as Error).message)}</Empty>
      ) : !stats ? (
        <Empty>compiling vibes…</Empty>
      ) : (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_260px]">
          <div className="min-w-0">
            {stats.series.length === 0 ? (
              <Empty>
                Starts {fmtDay(stats.sprint.starts_at.slice(0, 10))}. The chart fills in once the
                sprint is running.
              </Empty>
            ) : tab === "burndown" ? (
              <BurndownView stats={stats} />
            ) : (
              <BurnupView stats={stats} />
            )}
          </div>
          <SidePanel stats={stats} />
        </div>
      )}
    </section>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="mono rounded border border-dashed border-white/10 p-8 text-center text-xs text-chrome-dim">
      {children}
    </div>
  );
}

function unitLabel(stats: SprintStats) {
  return stats.unit === "points" ? "story points" : "tasks";
}

function BurndownView({ stats }: { stats: SprintStats }) {
  const data = stats.series.map((d) => ({
    day: fmtDay(d.date),
    remaining: d.remaining,
    ideal: d.ideal,
  }));
  return (
    <div className="h-72" data-chart="burndown">
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 4 }}>
          <defs>
            <linearGradient id="burnFill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#7c5cff" stopOpacity={0.35} />
              <stop offset="100%" stopColor="#7c5cff" stopOpacity={0.02} />
            </linearGradient>
          </defs>
          <CartesianGrid stroke="#ffffff10" strokeDasharray="3 3" />
          <XAxis dataKey="day" stroke="#9b9ba3" tick={AXIS} />
          <YAxis
            stroke="#9b9ba3"
            tick={AXIS}
            allowDecimals={false}
            label={{
              value: `remaining ${unitLabel(stats)}`,
              angle: -90,
              position: "insideLeft",
              style: { ...AXIS, fill: "#9b9ba3", textAnchor: "middle" },
            }}
          />
          <Tooltip {...TOOLTIP} />
          <Legend wrapperStyle={AXIS} />
          <Area
            type="linear"
            dataKey="remaining"
            name="actual remaining"
            stroke="#7c5cff"
            strokeWidth={2}
            fill="url(#burnFill)"
            dot={{ r: 3, fill: "#7c5cff" }}
            connectNulls={false}
            isAnimationActive={false}
          />
          <Line
            type="linear"
            dataKey="ideal"
            name="ideal burndown"
            stroke="#9b9ba3"
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            isAnimationActive={false}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}

function BurnupView({ stats }: { stats: SprintStats }) {
  const start = stats.series[0]?.ideal ?? 0;
  const data = stats.series.map((d) => ({
    day: fmtDay(d.date),
    done: d.done,
    scope: d.scope,
    ideal: Math.round((start - d.ideal) * 10) / 10,
  }));
  return (
    <div className="h-72" data-chart="burnup">
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 4 }}>
          <CartesianGrid stroke="#ffffff10" strokeDasharray="3 3" />
          <XAxis dataKey="day" stroke="#9b9ba3" tick={AXIS} />
          <YAxis
            stroke="#9b9ba3"
            tick={AXIS}
            allowDecimals={false}
            label={{
              value: unitLabel(stats),
              angle: -90,
              position: "insideLeft",
              style: { ...AXIS, fill: "#9b9ba3", textAnchor: "middle" },
            }}
          />
          <Tooltip {...TOOLTIP} />
          <Legend wrapperStyle={AXIS} />
          <Line
            type="stepAfter"
            dataKey="scope"
            name="scope"
            stroke="#f59e0b"
            strokeWidth={2}
            dot={false}
            isAnimationActive={false}
          />
          <Line
            type="linear"
            dataKey="done"
            name="completed"
            stroke="#10b981"
            strokeWidth={2}
            dot={{ r: 3, fill: "#10b981" }}
            isAnimationActive={false}
          />
          <Line
            type="linear"
            dataKey="ideal"
            name="ideal pace"
            stroke="#9b9ba3"
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            isAnimationActive={false}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}

function SidePanel({ stats }: { stats: SprintStats }) {
  const p = stats.progress;
  const pct = percent(p.tasks_done, p.tasks_total);
  const usePoints = stats.unit === "points";
  const total = usePoints ? p.points_total : p.tasks_total;
  const done = usePoints ? p.points_done : p.tasks_done;
  const unit = usePoints ? "pts" : "tasks";
  const sc = stats.scope;
  return (
    <div className="space-y-3">
      <div className="rounded border border-white/10 p-3" data-kpi="sprint-progress">
        <div className="mono mb-2 text-[10px] uppercase tracking-widest text-chrome-dim">
          sprint progress
        </div>
        <div className="flex items-center gap-4">
          <ProgressRing value={pct} label={`${pct}% of sprint tasks done`} />
          <dl className="mono space-y-1 text-xs">
            <dt className="text-chrome-dim">completed</dt>
            <dd className="text-base text-emerald-400">
              {p.tasks_done} / {p.tasks_total}
            </dd>
            <dt className="text-chrome-dim">remaining</dt>
            <dd className="text-base text-chrome">{p.tasks_total - p.tasks_done}</dd>
          </dl>
        </div>
      </div>

      <div className="rounded border border-white/10 p-3" data-kpi="key-metrics">
        <div className="mono mb-2 text-[10px] uppercase tracking-widest text-chrome-dim">
          key metrics
        </div>
        <dl className="mono grid grid-cols-[1fr_auto] gap-y-1 text-xs">
          <dt className="text-chrome-dim">total {usePoints ? "story points" : "tasks"}</dt>
          <dd className="text-right text-chrome">{total}</dd>
          <dt className="text-chrome-dim">completed</dt>
          <dd className="text-right text-chrome">{done}</dd>
          <dt className="text-chrome-dim">remaining</dt>
          <dd className="text-right text-chrome">{total - done}</dd>
          <dt className="text-chrome-dim">days elapsed</dt>
          <dd className="text-right text-chrome">
            {stats.days.elapsed} / {stats.days.total}
          </dd>
          <dt className="text-chrome-dim" title="work added after the sprint started ÷ the scope it started with">
            scope change
          </dt>
          <dd className="text-right text-chrome" data-kpi="scope-change">
            {sc && sc.change_percent != null
              ? `${sc.approximate ? "≈" : ""}+${sc.change_percent}%`
              : "—"}
          </dd>
        </dl>
        {sc && (sc.added_tasks > 0 || sc.removed_tasks > 0) && (
          <div className="mono mt-1 text-[10px] text-chrome-dim">
            +{sc.added_tasks} added · −{sc.removed_tasks} removed since the start
            {usePoints && ` (${sc.added_points}/${sc.original_points} ${unit})`}
          </div>
        )}
      </div>

      <div className="rounded border border-white/10 p-3" data-kpi="quick-view">
        <div className="mono mb-2 text-[10px] uppercase tracking-widest text-chrome-dim">
          quick view
        </div>
        <ul className="mono space-y-1 text-xs">
          <HealthRow color="#10b981" label="on track" n={stats.health.on_track} />
          <HealthRow color="#f59e0b" label="at risk" n={stats.health.at_risk} hint="overdue or due within two days" />
          <HealthRow color="#ef4444" label="blocked" n={stats.health.blocked} hint="waiting on an unfinished task" />
        </ul>
      </div>
    </div>
  );
}

function HealthRow({ color, label, n, hint }: { color: string; label: string; n: number; hint?: string }) {
  return (
    <li className="flex items-center gap-2" title={hint}>
      <span className="h-2 w-2 rounded-full" style={{ background: color }} aria-hidden />
      <span className="text-chrome-dim">{label}</span>
      <span className="ml-auto text-chrome">{n}</span>
    </li>
  );
}

function VelocityPanel({ projectKey }: { projectKey: string }) {
  const [unit, setUnit] = useState<VelocityUnit>("points");
  const q = useQuery({
    queryKey: ["velocity", projectKey],
    queryFn: () => getVelocity(projectKey),
  });
  if (q.error) return <Empty>Couldn&apos;t load velocity — {String((q.error as Error).message)}</Empty>;
  if (!q.data) return <Empty>compiling vibes…</Empty>;
  const { sprints, current } = q.data;
  const suffix = VELOCITY_UNITS.find((u) => u.unit === unit)!.suffix;
  const sum = summarizeVelocity(sprints, current, unit);
  const data = sprints.map((s) => ({
    name: s.name.length > 12 ? `${s.name.slice(0, 11)}…` : s.name,
    value: velocityValue(s, unit),
  }));

  return (
    <div className="space-y-3" data-chart="velocity">
      <div className="flex flex-wrap items-center gap-2">
        <Chip value={sum.current} suffix={suffix} label={sum.currentLabel ? `current — ${sum.currentLabel}` : "current sprint"} />
        <Chip value={sum.average} suffix={`${suffix}/sprint`} label="average velocity" />
        <span className="mono rounded border border-white/10 px-2 py-1 text-xs">
          <span className={sum.vsPrevious == null ? "text-chrome-dim" : sum.vsPrevious >= 0 ? "text-emerald-400" : "text-amber-300"}>
            {sum.vsPrevious == null ? "—" : signedPercent(sum.vsPrevious)}
          </span>
          <span className="ml-1 text-chrome-dim">vs. previous sprint</span>
        </span>
        <label className="mono ml-auto flex items-center gap-1 text-[11px] text-chrome-dim">
          measure in
          <select
            value={unit}
            onChange={(e) => setUnit(e.target.value as VelocityUnit)}
            aria-label="velocity unit"
            className="rounded border border-white/10 bg-ink px-1 py-0.5 text-chrome"
          >
            {VELOCITY_UNITS.map((u) => (
              <option key={u.unit} value={u.unit}>
                {u.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {sprints.length === 0 ? (
        <Empty>No completed sprints yet. Velocity is what a finished sprint leaves behind.</Empty>
      ) : (
        <>
          <p className="mono text-[11px] text-chrome-dim">
            Completed {VELOCITY_UNITS.find((u) => u.unit === unit)!.label} per sprint, with the
            team&apos;s average as the dashed line.
          </p>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 4 }}>
                <CartesianGrid stroke="#ffffff10" strokeDasharray="3 3" vertical={false} />
                <XAxis dataKey="name" stroke="#9b9ba3" tick={AXIS} />
                <YAxis stroke="#9b9ba3" tick={AXIS} allowDecimals={unit === "hours" || unit === "logged"} />
                <Tooltip {...TOOLTIP} formatter={(v: number) => [`${v} ${suffix}`, "completed"]} />
                {sum.average != null && (
                  <ReferenceLine
                    y={sum.average}
                    stroke="#f59e0b"
                    strokeDasharray="5 5"
                    label={{ value: `avg ${sum.average}`, position: "insideTopRight", fill: "#f59e0b", ...AXIS }}
                  />
                )}
                <Bar dataKey="value" fill="#5b8def" radius={[4, 4, 0, 0]} isAnimationActive={false} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </>
      )}
    </div>
  );
}

function Chip({ value, suffix, label }: { value: number | null; suffix: string; label: string }) {
  return (
    <span className="mono rounded border border-white/10 px-2 py-1 text-xs">
      <span className="text-chrome">{value == null ? "—" : `${value} ${suffix}`}</span>
      <span className="ml-1 text-chrome-dim">{label}</span>
    </span>
  );
}
