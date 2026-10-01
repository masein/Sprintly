"use client";

// The team on the project dashboard (KPI request alongside QA report 6:
// "there is no way to see others' KPIs, clockwork in the dashboard of the
// project"). Two views:
//
//   • clockwork — members × days, hours logged, for this week / last week /
//     this month / last month / any range from the calendar;
//   • KPIs — per member, for the running (or last) sprint: completion rate,
//     on-time completion rate, estimation rate.
//
// Leads see everyone's hours; others see their own and a dot for the rest.

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Clock, Gauge } from "lucide-react";
import {
  RANGE_PRESETS,
  cellHours,
  getTeam,
  presetRange,
  rate,
  type RangePreset,
  type Team,
} from "@/lib/team";
import { fmtMinutes } from "@/lib/timetracking";
import { Avatar } from "./Avatar";

type View = "clockwork" | "kpis";

const weekday = (iso: string) =>
  new Date(`${iso}T00:00:00Z`).toLocaleDateString("en", { weekday: "short", timeZone: "UTC" });
const dayNum = (iso: string) => Number(iso.slice(8, 10));

export function TeamPanel({ projectKey }: { projectKey: string }) {
  const [view, setView] = useState<View>("clockwork");
  const [preset, setPreset] = useState<RangePreset | "custom">("this-week");
  const [custom, setCustom] = useState(() => presetRange("this-week", new Date()));
  const range = preset === "custom" ? custom : presetRange(preset, new Date());

  const q = useQuery({
    queryKey: ["team", projectKey, range.from, range.to],
    queryFn: () => getTeam(projectKey, range.from, range.to),
    enabled: !!projectKey && range.from <= range.to,
  });

  return (
    <section aria-label="team" className="rounded-lg border border-white/10 bg-ink-subtle p-4" data-team-panel>
      <header className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h2 className="font-semibold">Team</h2>
        <div role="tablist" aria-label="team view" className="flex rounded border border-white/10 p-0.5">
          {([
            ["clockwork", Clock],
            ["kpis", Gauge],
          ] as const).map(([v, Icon]) => (
            <button
              key={v}
              type="button"
              role="tab"
              aria-selected={view === v}
              onClick={() => setView(v)}
              className={`mono flex items-center gap-1 rounded px-2.5 py-1 text-xs transition ${
                view === v ? "bg-accent/20 text-chrome" : "text-chrome-dim hover:text-chrome"
              }`}
            >
              <Icon size={11} /> {v === "kpis" ? "KPIs" : v}
            </button>
          ))}
        </div>
      </header>

      {view === "clockwork" && (
        <div className="mb-3 flex flex-wrap items-center gap-1.5" aria-label="date range">
          {RANGE_PRESETS.map((p) => (
            <button
              key={p.id}
              type="button"
              aria-pressed={preset === p.id}
              onClick={() => setPreset(p.id)}
              className={`mono rounded border px-2 py-0.5 text-[11px] ${
                preset === p.id
                  ? "border-accent/60 bg-accent/10 text-chrome"
                  : "border-white/10 text-chrome-dim hover:text-chrome"
              }`}
            >
              {p.label}
            </button>
          ))}
          <span className="mono ml-1 flex items-center gap-1 text-[11px] text-chrome-dim">
            <input
              type="date"
              aria-label="from"
              value={range.from}
              max={range.to}
              onChange={(e) => {
                setPreset("custom");
                setCustom({ from: e.target.value, to: range.to });
              }}
              className="rounded border border-white/10 bg-ink px-1 py-0.5 text-chrome"
            />
            →
            <input
              type="date"
              aria-label="to"
              value={range.to}
              min={range.from}
              onChange={(e) => {
                setPreset("custom");
                setCustom({ from: range.from, to: e.target.value });
              }}
              className="rounded border border-white/10 bg-ink px-1 py-0.5 text-chrome"
            />
          </span>
        </div>
      )}

      {q.error ? (
        <div className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-[11px] text-red-200">
          {(q.error as Error).message}
        </div>
      ) : !q.data ? (
        <div className="mono text-[11px] text-chrome-dim">crunching the numbers…</div>
      ) : view === "clockwork" ? (
        <Clockwork team={q.data} />
      ) : (
        <Kpis team={q.data} />
      )}
    </section>
  );
}

function Clockwork({ team }: { team: Team }) {
  const max = Math.max(1, ...team.members.flatMap((m) => m.minutes_by_day ?? [0]));
  const grand = team.totals_by_day.reduce((a, b) => a + b, 0);
  return (
    <>
      <div className="overflow-x-auto">
        <table className="mono w-full border-collapse text-[11px]" data-clockwork>
          <thead>
            <tr className="text-chrome-dim">
              <th className="sticky left-0 z-10 bg-ink-subtle py-1 pr-3 text-left font-normal">member</th>
              {team.days.map((d) => (
                <th key={d} className="min-w-[2.75rem] px-1 py-1 text-center font-normal" title={d}>
                  <div className="text-[9px] uppercase">{weekday(d)}</div>
                  <div>{dayNum(d)}</div>
                </th>
              ))}
              <th className="px-2 py-1 text-right font-normal">total</th>
            </tr>
          </thead>
          <tbody>
            {team.members.map((m) => (
              <tr key={m.user_id} className="border-t border-white/5" data-clockwork-row={m.handle}>
                <td className="sticky left-0 z-10 bg-ink-subtle py-1 pr-3">
                  <span className="flex items-center gap-1.5 whitespace-nowrap">
                    <Avatar
                      size={16}
                      user={{
                        userId: m.user_id,
                        displayName: m.display_name,
                        handle: m.handle,
                        avatarUrl: m.avatar_url,
                        avatarStyle: m.avatar_style,
                        avatarSeed: m.avatar_seed,
                      }}
                    />
                    <span className="text-chrome">@{m.handle}</span>
                  </span>
                </td>
                {team.days.map((d, i) => {
                  const v = m.minutes_by_day?.[i] ?? null;
                  return (
                    <td
                      key={d}
                      className="px-0.5 py-0.5 text-center"
                      title={v == null ? "only leads see others' hours" : `${m.handle} · ${d} · ${fmtMinutes(v)}`}
                    >
                      {v == null ? (
                        <span className="text-chrome-dim/40">·</span>
                      ) : (
                        <span
                          className="block rounded px-1 py-0.5 text-chrome"
                          style={{
                            background: v > 0 ? `rgba(124, 92, 255, ${0.12 + 0.5 * (v / max)})` : undefined,
                          }}
                        >
                          {cellHours(v)}
                        </span>
                      )}
                    </td>
                  );
                })}
                <td className="px-2 py-1 text-right text-chrome">
                  {m.total_minutes == null ? "·" : fmtMinutes(m.total_minutes)}
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="border-t border-white/10 text-chrome-dim">
              <td className="sticky left-0 z-10 bg-ink-subtle py-1 pr-3">all</td>
              {team.totals_by_day.map((v, i) => (
                <td key={team.days[i]} className="px-1 py-1 text-center">
                  {cellHours(v)}
                </td>
              ))}
              <td className="px-2 py-1 text-right text-chrome">{fmtMinutes(grand)}</td>
            </tr>
          </tfoot>
        </table>
      </div>
      {team.scope === "self" && (
        <div className="mono mt-2 text-[10px] text-chrome-dim">
          your hours only — leads see the whole team
        </div>
      )}
      {grand === 0 && (
        <div className="mono mt-2 text-[11px] text-chrome-dim">
          Either nobody worked or everyone forgot the timer. Both are valid.
        </div>
      )}
    </>
  );
}

function Kpis({ team }: { team: Team }) {
  if (!team.sprint) {
    return (
      <div className="mono rounded border border-dashed border-white/10 p-6 text-center text-xs text-chrome-dim">
        No sprint to measure yet. Start one and each person&apos;s completion, on-time and
        estimation rates show up here.
      </div>
    );
  }
  const rows = team.members.filter((m) => m.kpi.assigned > 0 || m.role !== "watcher");
  return (
    <>
      <p className="mono mb-2 text-[11px] text-chrome-dim">
        {team.sprint.name}
        {team.sprint.state === "completed" ? " (completed)" : " (running)"} · completion = done ÷
        assigned · on time = done by the due date ÷ done with one · estimated = with points or an
        estimate ÷ assigned
      </p>
      <div className="overflow-x-auto">
        <table className="mono w-full border-collapse text-[11px]" data-kpi-table>
          <thead>
            <tr className="text-chrome-dim">
              <th className="py-1 pr-3 text-left font-normal">member</th>
              <th className="px-2 py-1 text-right font-normal">tasks</th>
              <th className="px-2 py-1 text-left font-normal">completion</th>
              <th className="px-2 py-1 text-left font-normal">on time</th>
              <th className="px-2 py-1 text-left font-normal">estimated</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((m) => (
              <tr key={m.user_id} className="border-t border-white/5" data-kpi-row={m.handle}>
                <td className="whitespace-nowrap py-1.5 pr-3 text-chrome">@{m.handle}</td>
                <td className="px-2 py-1.5 text-right text-chrome-dim">{m.kpi.assigned}</td>
                <RateCell
                  value={rate(m.kpi.completed, m.kpi.assigned)}
                  detail={`${m.kpi.completed}/${m.kpi.assigned}`}
                  color="#10b981"
                />
                <RateCell
                  value={rate(m.kpi.on_time, m.kpi.completed_with_due)}
                  detail={
                    m.kpi.completed_with_due > 0
                      ? `${m.kpi.on_time}/${m.kpi.completed_with_due} with a due date`
                      : "no due dates"
                  }
                  color="#22d3ee"
                />
                <RateCell
                  value={rate(m.kpi.estimated, m.kpi.assigned)}
                  detail={`${m.kpi.estimated}/${m.kpi.assigned}`}
                  color="#f59e0b"
                />
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

function RateCell({ value, detail, color }: { value: number | null; detail: string; color: string }) {
  return (
    <td className="min-w-[8rem] px-2 py-1.5" title={detail}>
      {value == null ? (
        <span className="text-chrome-dim">—</span>
      ) : (
        <span className="flex items-center gap-2">
          <span className="h-1.5 w-16 overflow-hidden rounded bg-white/10" aria-hidden>
            <span className="block h-full" style={{ width: `${value}%`, background: color }} />
          </span>
          <span className="text-chrome">{value}%</span>
          <span className="text-[10px] text-chrome-dim">{detail}</span>
        </span>
      )}
    </td>
  );
}
