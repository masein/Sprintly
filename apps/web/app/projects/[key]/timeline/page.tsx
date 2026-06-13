"use client";

// F6 — Roadmap / timeline. A Gantt-lite view: epics as coloured bars over a
// date axis (with done/total progress), milestones as dated markers. Below the
// chart, manage epics + milestones. Drag-to-reschedule is deliberately out of
// scope (v2) — edit an epic's dates in its row instead.

import { useMemo, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { Flag, Plus, Trash2 } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { getProject } from "@/lib/projects";
import {
  createEpic,
  createMilestone,
  deleteEpic,
  deleteMilestone,
  listEpics,
  listMilestones,
  updateEpic,
  type Epic,
  type Milestone,
} from "@/lib/roadmap";
import type { ApiError } from "@/lib/api";

const SWATCHES = ["#7c5cff", "#22d3ee", "#10b981", "#f59e0b", "#ef4444", "#ec4899"];
const DAY = 86_400_000;

const parseDate = (s: string) => Date.parse(`${s}T00:00:00Z`);
const fmtMonth = (ms: number) =>
  new Date(ms).toLocaleString("en", { month: "short", year: "2-digit", timeZone: "UTC" });

export default function TimelinePage() {
  const params = useParams<{ key: string }>();
  const key = params?.key ?? "";
  const router = useRouter();
  const qc = useQueryClient();

  const projectQ = useQuery({ queryKey: ["project", key], queryFn: () => getProject(key) });
  const epicsQ = useQuery({ queryKey: ["epics", key], queryFn: () => listEpics(key), retry: false });
  const msQ = useQuery({ queryKey: ["milestones", key], queryFn: () => listMilestones(key) });

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ["epics", key] });
    qc.invalidateQueries({ queryKey: ["milestones", key] });
  };

  if (epicsQ.error) {
    const e = epicsQ.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    if (e.status === 403) {
      return (
        <AppShell currentProjectKey={key}>
          <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
            You don&apos;t have access to this project.
          </div>
        </AppShell>
      );
    }
  }

  const canManage = projectQ.data?.your_role === "lead";
  const epics = epicsQ.data ?? [];
  const milestones = msQ.data ?? [];

  return (
    <AppShell currentProjectKey={key}>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            {key} · roadmap
          </div>
          <h1 className="text-3xl font-semibold">The shape of the quarter.</h1>
        </div>
        <Link href={`/projects/${key}`} className="mono text-xs text-accent hover:underline">
          ← board
        </Link>
      </header>

      <Timeline epics={epics} milestones={milestones} />

      <div className="mt-8 grid grid-cols-1 gap-6 lg:grid-cols-2">
        <EpicsManager
          projectKey={key}
          epics={epics}
          canManage={canManage}
          onChange={invalidate}
        />
        <MilestonesManager
          projectKey={key}
          milestones={milestones}
          canManage={canManage}
          onChange={invalidate}
        />
      </div>
    </AppShell>
  );
}

// ─── Timeline (Gantt-lite) ───────────────────────────────────────────────────

function Timeline({ epics, milestones }: { epics: Epic[]; milestones: Milestone[] }) {
  const scheduled = epics.filter((e) => e.start_date && e.end_date);

  const window = useMemo(() => {
    const dates: number[] = [];
    for (const e of scheduled) {
      dates.push(parseDate(e.start_date!), parseDate(e.end_date!));
    }
    for (const m of milestones) dates.push(parseDate(m.due_date));
    if (dates.length === 0) return null;
    const min = Math.min(...dates) - 3 * DAY;
    const max = Math.max(...dates) + 3 * DAY;
    return { min, max, span: Math.max(max - min, DAY) };
  }, [scheduled, milestones]);

  if (!window) {
    return (
      <div className="mono rounded-lg border border-dashed border-white/10 bg-ink-subtle p-10 text-center text-sm text-chrome-dim">
        Nothing scheduled yet. Give an epic a start and end date, or drop a
        milestone, and the timeline draws itself.
      </div>
    );
  }

  const pos = (ms: number) => Math.min(100, Math.max(0, ((ms - window.min) / window.span) * 100));

  // Month gridlines across the window.
  const months: { left: number; label: string }[] = [];
  {
    const d = new Date(window.min);
    d.setUTCDate(1);
    d.setUTCMonth(d.getUTCMonth() + 1);
    while (d.getTime() <= window.max) {
      months.push({ left: pos(d.getTime()), label: fmtMonth(d.getTime()) });
      d.setUTCMonth(d.getUTCMonth() + 1);
    }
  }

  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      {/* axis */}
      <div className="relative mb-2 h-4">
        {months.map((m) => (
          <span
            key={m.label}
            className="mono absolute -translate-x-1/2 text-[10px] text-chrome-dim"
            style={{ left: `${m.left}%` }}
          >
            {m.label}
          </span>
        ))}
      </div>

      <div className="relative">
        {/* gridlines + milestone markers span the whole bar area */}
        {months.map((m) => (
          <div
            key={`grid-${m.label}`}
            aria-hidden
            className="absolute top-0 bottom-0 w-px bg-white/5"
            style={{ left: `${m.left}%` }}
          />
        ))}
        {milestones.map((m) => (
          <div
            key={m.id}
            className="absolute top-0 bottom-0 z-10 flex flex-col items-center"
            style={{ left: `${pos(parseDate(m.due_date))}%` }}
            title={`${m.name} · ${m.due_date}`}
          >
            <div className="w-px flex-1 bg-amber-400/50" />
            <Flag size={11} className="absolute -top-1 -translate-y-full text-amber-300" />
          </div>
        ))}

        {/* epic bars */}
        <div className="relative space-y-1.5 py-1">
          {scheduled.map((e) => {
            const left = pos(parseDate(e.start_date!));
            const right = pos(parseDate(e.end_date!));
            const width = Math.max(right - left, 1.5);
            const pct = e.task_count > 0 ? Math.round((e.done_count / e.task_count) * 100) : 0;
            return (
              <div key={e.id} className="relative h-7">
                <div
                  data-testid="epic-bar"
                  className="absolute top-0 flex h-7 items-center overflow-hidden rounded"
                  style={{ left: `${left}%`, width: `${width}%`, background: `${e.color}33`, border: `1px solid ${e.color}` }}
                  title={`${e.name} · ${e.done_count}/${e.task_count} done`}
                >
                  {/* progress fill */}
                  <div
                    aria-hidden
                    className="absolute inset-y-0 left-0"
                    style={{ width: `${pct}%`, background: `${e.color}55` }}
                  />
                  <span className="mono relative truncate px-2 text-[11px] text-chrome">
                    {e.name}
                    <span className="ml-1 text-chrome-dim">
                      {e.done_count}/{e.task_count}
                    </span>
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {scheduled.length === 0 && (
        <div className="mono pt-2 text-center text-[11px] text-chrome-dim">
          milestones only — schedule an epic to see a bar
        </div>
      )}
    </div>
  );
}

// ─── Epics management ────────────────────────────────────────────────────────

function EpicsManager({
  projectKey,
  epics,
  canManage,
  onChange,
}: {
  projectKey: string;
  epics: Epic[];
  canManage: boolean;
  onChange: () => void;
}) {
  const [name, setName] = useState("");
  const [color, setColor] = useState(SWATCHES[0]!);
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () =>
      createEpic(projectKey, {
        name: name.trim(),
        color,
        start_date: start || null,
        end_date: end || null,
      }),
    onSuccess: () => {
      setName("");
      setStart("");
      setEnd("");
      setError(null);
      onChange();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const remove = useMutation({ mutationFn: (id: string) => deleteEpic(id), onSuccess: onChange });

  return (
    <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">epics</h2>
      <ul className="space-y-1">
        {epics.map((e) => (
          <EpicRow key={e.id} epic={e} canManage={canManage} onChange={onChange} onDelete={() => remove.mutate(e.id)} />
        ))}
        {epics.length === 0 && <li className="mono text-[11px] text-chrome-dim">no epics yet</li>}
      </ul>

      {canManage && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) add.mutate();
          }}
          className="space-y-2 border-t border-white/10 pt-3"
        >
          <div className="flex items-center gap-2">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={80}
              placeholder="new epic"
              aria-label="epic name"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <button
              type="submit"
              disabled={add.isPending || !name.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              add
            </button>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <input type="date" aria-label="epic start" value={start} onChange={(e) => setStart(e.target.value)} className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome" />
            <span className="mono text-[11px] text-chrome-dim">→</span>
            <input type="date" aria-label="epic end" value={end} onChange={(e) => setEnd(e.target.value)} className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome" />
            <div className="flex items-center gap-1">
              {SWATCHES.map((s) => (
                <button
                  key={s}
                  type="button"
                  aria-label={`color ${s}`}
                  onClick={() => setColor(s)}
                  style={{ background: s }}
                  className={`h-4 w-4 rounded-full border-2 ${color === s ? "border-white" : "border-transparent"}`}
                />
              ))}
            </div>
          </div>
          {error && <div className="mono text-[11px] text-red-300">{error}</div>}
        </form>
      )}
    </section>
  );
}

function EpicRow({
  epic,
  canManage,
  onChange,
  onDelete,
}: {
  epic: Epic;
  canManage: boolean;
  onChange: () => void;
  onDelete: () => void;
}) {
  const save = useMutation({
    mutationFn: (body: Partial<Epic>) => updateEpic(epic.id, body),
    onSuccess: onChange,
  });
  const pct = epic.task_count > 0 ? Math.round((epic.done_count / epic.task_count) * 100) : 0;
  return (
    <li className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5">
      <span className="h-3 w-3 shrink-0 rounded-sm" style={{ background: epic.color }} aria-hidden />
      <span className="mono truncate text-xs text-chrome">{epic.name}</span>
      <span className="mono text-[10px] text-chrome-dim">
        {epic.done_count}/{epic.task_count}
        {epic.task_count > 0 ? ` · ${pct}%` : ""}
      </span>
      {canManage && (
        <span className="ml-auto flex items-center gap-1">
          <input
            type="date"
            aria-label={`${epic.name} start`}
            defaultValue={epic.start_date ?? ""}
            onChange={(e) => save.mutate({ start_date: e.target.value || null })}
            className="mono rounded border border-white/10 bg-ink px-1 py-0.5 text-[10px] text-chrome"
          />
          <input
            type="date"
            aria-label={`${epic.name} end`}
            defaultValue={epic.end_date ?? ""}
            onChange={(e) => save.mutate({ end_date: e.target.value || null })}
            className="mono rounded border border-white/10 bg-ink px-1 py-0.5 text-[10px] text-chrome"
          />
          <button
            type="button"
            aria-label={`delete ${epic.name}`}
            onClick={() => {
              if (confirm(`Delete the "${epic.name}" epic? Its tasks stay, just unassigned.`)) onDelete();
            }}
            className="text-chrome-dim hover:text-red-300"
          >
            <Trash2 size={13} />
          </button>
        </span>
      )}
    </li>
  );
}

// ─── Milestones management ───────────────────────────────────────────────────

function MilestonesManager({
  projectKey,
  milestones,
  canManage,
  onChange,
}: {
  projectKey: string;
  milestones: Milestone[];
  canManage: boolean;
  onChange: () => void;
}) {
  const [name, setName] = useState("");
  const [due, setDue] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () => createMilestone(projectKey, { name: name.trim(), due_date: due }),
    onSuccess: () => {
      setName("");
      setDue("");
      setError(null);
      onChange();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const remove = useMutation({ mutationFn: (id: string) => deleteMilestone(id), onSuccess: onChange });

  return (
    <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">milestones</h2>
      <ul className="space-y-1">
        {milestones.map((m) => (
          <li key={m.id} className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5">
            <Flag size={12} className="shrink-0 text-amber-300" />
            <span className="mono truncate text-xs text-chrome">{m.name}</span>
            <span className="mono ml-auto text-[10px] text-chrome-dim">{m.due_date}</span>
            {canManage && (
              <button
                type="button"
                aria-label={`delete ${m.name}`}
                onClick={() => remove.mutate(m.id)}
                className="text-chrome-dim hover:text-red-300"
              >
                <Trash2 size={13} />
              </button>
            )}
          </li>
        ))}
        {milestones.length === 0 && (
          <li className="mono text-[11px] text-chrome-dim">no milestones — no deadlines, no problems</li>
        )}
      </ul>

      {canManage && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim() && due) add.mutate();
          }}
          className="flex flex-wrap items-center gap-2 border-t border-white/10 pt-3"
        >
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={80}
            placeholder="new milestone"
            aria-label="milestone name"
            className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          <input
            type="date"
            aria-label="milestone due"
            value={due}
            onChange={(e) => setDue(e.target.value)}
            className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
          />
          <button
            type="submit"
            disabled={add.isPending || !name.trim() || !due}
            className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
          >
            <Plus size={11} className="-mt-0.5 mr-0.5 inline" /> add
          </button>
          {error && <div className="mono w-full text-[11px] text-red-300">{error}</div>}
        </form>
      )}
    </section>
  );
}
