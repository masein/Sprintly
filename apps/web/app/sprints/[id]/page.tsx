"use client";

// Sprint detail. Header (name/goal/dates/state) + actions (start/complete) +
// task assignment + burndown chart + summary (when retro is closed).

import { useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, CheckCircle2, Plus, Trash2, X } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { BurndownChart } from "@/components/BurndownChart";
import { Markdown } from "@/components/Markdown";
import {
  assignTaskToSprint,
  completeSprint,
  getBurndown,
  getSprint,
  listSprintTasks,
  startSprint,
  unassignTaskFromSprint,
  type SprintTask,
} from "@/lib/sprints";
import { search } from "@/lib/search";
import { createTask } from "@/lib/tasks";
import { pluralize } from "@/lib/format";
import type { ApiError } from "@/lib/api";

export default function SprintDetailPage() {
  const router = useRouter();
  const params = useParams<{ id: string }>();
  const id = params?.id ?? "";
  const qc = useQueryClient();

  const sprintQ = useQuery({
    queryKey: ["sprint", id],
    queryFn: () => getSprint(id),
    enabled: !!id,
  });
  const tasksQ = useQuery({
    queryKey: ["sprint-tasks", id],
    queryFn: () => listSprintTasks(id),
    enabled: !!id,
  });
  const burnQ = useQuery({
    queryKey: ["sprint-burndown", id],
    queryFn: () => getBurndown(id),
    enabled: !!id,
  });

  const start = useMutation({
    mutationFn: () => startSprint(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sprint", id] }),
    onError: (e) => alert((e as unknown as ApiError).message),
  });
  const complete = useMutation({
    mutationFn: () => completeSprint(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sprint", id] });
      // Confetti is allowed: closing a sprint (per docs/PERSONALITY.md).
      void import("@/lib/confetti").then((m) => m.fire(120));
      // Brief pause so the user actually sees the confetti before nav.
      setTimeout(() => router.push(`/sprints/${id}/retro`), 1100);
    },
    onError: (e) => alert((e as unknown as ApiError).message),
  });

  if (sprintQ.error) {
    const e = sprintQ.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
  }

  const sprint = sprintQ.data;
  if (!sprint) {
    return (
      <AppShell>
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      </AppShell>
    );
  }

  return (
    <AppShell currentProjectKey={sprint.project_key}>
      <div className="mb-4 flex items-center gap-3">
        <Link
          href={`/projects/${sprint.project_key}/sprints`}
          className="mono text-xs text-chrome-dim hover:text-chrome"
        >
          ← {sprint.project_key} · sprints
        </Link>
        <span
          className={`mono inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${
            sprint.state === "active"
              ? "border-accent bg-accent/10 text-accent"
              : "border-white/10 text-chrome-dim"
          }`}
        >
          {sprint.state}
        </span>
        {sprint.state === "completed" && (
          <Link
            href={`/sprints/${id}/retro`}
            className="mono text-xs text-accent hover:underline"
          >
            → retro
          </Link>
        )}
      </div>

      <header className="mb-6">
        <h1 className="text-3xl font-semibold">{sprint.name}</h1>
        <div className="mono mt-1 text-xs text-chrome-dim">
          {sprint.starts_at.slice(0, 10)} → {sprint.ends_at.slice(0, 10)} · {pluralize(sprint.task_count, "task")} · {sprint.done_points}/{sprint.total_points} pts
          {sprint.velocity_points != null && (
            <> · velocity {sprint.velocity_points}</>
          )}
        </div>
        {sprint.goal && (
          <section className="mt-3 rounded-lg border border-white/10 bg-ink-subtle p-3">
            <Markdown>{sprint.goal}</Markdown>
          </section>
        )}
      </header>

      <div className="mb-4 flex items-center gap-2">
        {sprint.state === "planned" && (
          <button
            type="button"
            onClick={() => start.mutate()}
            disabled={start.isPending}
            className="mono inline-flex items-center gap-2 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
          >
            <Play size={14} /> start sprint
          </button>
        )}
        {sprint.state === "active" && (
          <button
            type="button"
            onClick={() => {
              if (!confirm("Complete this sprint? Opens the retro.")) return;
              complete.mutate();
            }}
            disabled={complete.isPending}
            className="mono inline-flex items-center gap-2 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
          >
            <CheckCircle2 size={14} /> complete + open retro
          </button>
        )}
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_360px]">
        <section>
          <h2 className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
            tasks ({tasksQ.data?.length ?? 0})
          </h2>
          <TaskList tasks={tasksQ.data ?? []} sprintId={id} canManage={sprint.state !== "completed"} />
          {sprint.state !== "completed" && (
            <AddTaskRow sprintId={id} projectKey={sprint.project_key} onAdded={() => {
              qc.invalidateQueries({ queryKey: ["sprint-tasks", id] });
              qc.invalidateQueries({ queryKey: ["sprint", id] });
              qc.invalidateQueries({ queryKey: ["sprint-burndown", id] });
            }} />
          )}
        </section>
        <aside>
          {burnQ.data && <BurndownChart points={burnQ.data.items} />}
          {sprint.summary_md && (
            <section className="mt-4 rounded-lg border border-white/10 bg-ink-subtle p-4">
              <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
                retro summary
              </div>
              <Markdown>{sprint.summary_md}</Markdown>
            </section>
          )}
        </aside>
      </div>
    </AppShell>
  );
}

function TaskList({
  tasks,
  sprintId,
  canManage,
}: {
  tasks: SprintTask[];
  sprintId: string;
  canManage: boolean;
}) {
  const qc = useQueryClient();
  const unassign = useMutation({
    mutationFn: (key: string) => unassignTaskFromSprint(sprintId, key),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sprint-tasks", sprintId] });
      qc.invalidateQueries({ queryKey: ["sprint", sprintId] });
      qc.invalidateQueries({ queryKey: ["sprint-burndown", sprintId] });
    },
  });
  if (tasks.length === 0) {
    return (
      <div className="mono rounded border border-dashed border-white/10 p-4 text-center text-xs text-chrome-dim">
        nothing in this sprint yet
      </div>
    );
  }
  return (
    <ul className="space-y-1">
      {tasks.map((t) => (
        <li
          key={t.key}
          className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2"
        >
          <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
            {t.status}
          </span>
          <Link
            href={`/tasks/${t.key}`}
            className="mono text-xs text-accent hover:underline"
          >
            {t.key}
          </Link>
          <span className="flex-1 truncate text-sm text-chrome">{t.title}</span>
          <span className="mono text-xs text-chrome-dim">
            {t.story_points != null ? `${t.story_points} pts` : "—"}
          </span>
          {canManage && (
            <button
              type="button"
              onClick={() => unassign.mutate(t.key)}
              className="text-chrome-dim hover:text-red-300"
              aria-label="Remove from sprint"
            >
              <Trash2 size={12} />
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

// Inline quick-add for a sprint (QA F1). Search existing tasks OR create a new
// one — typing a fresh title and pressing Enter creates it in this project and
// drops it into the sprint, then clears + keeps focus for the next one (mirrors
// the board's rapid quick-add). Enter is never a silent no-op.
function AddTaskRow({
  sprintId,
  projectKey,
  onAdded,
}: {
  sprintId: string;
  projectKey: string;
  onAdded: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [highlight, setHighlight] = useState(0);
  const [cue, setCue] = useState<{ msg: string; ok: boolean } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const hitsQ = useQuery({
    queryKey: ["sprint-task-search", q],
    queryFn: () => search(q, 6),
    enabled: q.trim().length >= 2,
    staleTime: 5_000,
  });
  const hits = hitsQ.data?.tasks ?? [];
  const trimmed = q.trim();
  const searched = q.trim().length >= 2 && !hitsQ.isFetching;
  // Offer "create" unless the query is an exact title match of an existing task.
  const exact = hits.some((t) => t.title.trim().toLowerCase() === trimmed.toLowerCase());
  const showCreate = trimmed.length > 0 && !exact;
  const rowCount = (showCreate ? 1 : 0) + hits.length;

  function flash(msg: string, ok: boolean) {
    setCue({ msg, ok });
    window.setTimeout(() => setCue((c) => (c?.msg === msg ? null : c)), 1800);
  }

  function afterAdd(msg: string) {
    onAdded();
    setQ("");
    setHighlight(0);
    flash(msg, true);
    inputRef.current?.focus();
  }

  const addExisting = useMutation({
    mutationFn: (key: string) => assignTaskToSprint(sprintId, key),
    onSuccess: (_d, key) => afterAdd(`added ${key}`),
    onError: (e) => flash((e as unknown as ApiError).message ?? "couldn't add it", false),
  });
  const createAndAdd = useMutation({
    mutationFn: (title: string) => createTask(projectKey, { title, sprint_id: sprintId }),
    onSuccess: (task) => afterAdd(`created ${task.key}`),
    onError: (e) => flash((e as unknown as ApiError).message ?? "couldn't create it", false),
  });
  const busy = addExisting.isPending || createAndAdd.isPending;

  function commit(index: number) {
    if (busy) return;
    if (showCreate && index === 0) {
      if (trimmed) createAndAdd.mutate(trimmed);
      return;
    }
    const hit = hits[index - (showCreate ? 1 : 0)];
    if (hit) addExisting.mutate(hit.key);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, Math.max(rowCount - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (rowCount === 0) return; // empty query — nothing to add or create
      commit(highlight < rowCount ? highlight : 0);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      setQ("");
    }
  }

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mono mt-2 flex items-center gap-1 text-xs text-chrome-dim hover:text-chrome"
      >
        <Plus size={12} /> add tasks
      </button>
    );
  }

  const createIndex = 0;
  return (
    <div className="mt-2 space-y-1 rounded border border-white/10 bg-ink-subtle p-2">
      <div className="flex items-center gap-2">
        <input
          ref={inputRef}
          autoFocus
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            setHighlight(0);
          }}
          onKeyDown={onKeyDown}
          placeholder="find a task, or type a new one…"
          aria-label="add a task to this sprint"
          className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
        />
        <button
          type="button"
          onClick={() => {
            setOpen(false);
            setQ("");
          }}
          className="text-chrome-dim hover:text-chrome"
          aria-label="Cancel"
        >
          <X size={12} />
        </button>
      </div>

      <ul className="max-h-48 overflow-y-auto">
        {showCreate && (
          <li>
            <button
              type="button"
              onMouseEnter={() => setHighlight(createIndex)}
              onClick={() => commit(createIndex)}
              className={`mono flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs ${
                highlight === createIndex ? "bg-accent/15 text-chrome" : "text-chrome-dim hover:bg-white/5"
              }`}
            >
              <span className="text-accent">↵</span>
              <span className="text-chrome">
                create &ldquo;<span className="truncate">{trimmed}</span>&rdquo;
              </span>
              <span className="ml-auto text-chrome-dim">new task</span>
            </button>
          </li>
        )}
        {hits.map((t, i) => {
          const idx = i + (showCreate ? 1 : 0);
          return (
            <li key={t.key}>
              <button
                type="button"
                onMouseEnter={() => setHighlight(idx)}
                onClick={() => commit(idx)}
                className={`mono flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs ${
                  highlight === idx ? "bg-accent/15" : "hover:bg-white/5"
                }`}
              >
                <span className="text-chrome-dim">{t.key}</span>
                <span className="truncate text-chrome">{t.title}</span>
                <span className="ml-auto text-chrome-dim">{t.status}</span>
              </button>
            </li>
          );
        })}
      </ul>

      <div className="mono px-1 text-[10px] text-chrome-dim">
        {busy
          ? "nudging electrons…"
          : cue
            ? <span className={cue.ok ? "text-emerald-300" : "text-red-300"}>{cue.msg}</span>
            : hitsQ.isFetching
              ? "searching…"
              : searched && hits.length === 0
                ? "nothing matches yet — ↵ to create it"
                : "↵ to add · ↑↓ to choose · esc to close"}
      </div>
    </div>
  );
}
