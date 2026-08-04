"use client";

// F9 — Backlog: unscheduled (no-sprint) tasks with multi-select bulk actions
// (assign, move to a sprint, delete). The board is for flow; this is for
// triaging the pile.

import { useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { CheckSquare, Plus, Square, Trash2, UserPlus, UserMinus } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Breadcrumbs, projectCrumbs } from "@/components/Breadcrumbs";
import { LoadError } from "@/components/LoadError";
import { getProject } from "@/lib/projects";
import { listSprints } from "@/lib/sprints";
import { createTask } from "@/lib/tasks";
import { me } from "@/lib/auth-bundle";
import { bulkTasks, listBacklog, type BacklogItem, type BulkOp } from "@/lib/templates";
import type { ApiError } from "@/lib/api";

const PRIO: Record<BacklogItem["priority"], { label: string; cls: string }> = {
  p0: { label: "p0", cls: "border-red-500/30 text-red-300" },
  p1: { label: "p1", cls: "border-amber-500/30 text-amber-300" },
  p2: { label: "p2", cls: "border-white/10 text-chrome-dim" },
  p3: { label: "p3", cls: "border-white/10 text-chrome-dim/70" },
};

export default function BacklogPage() {
  const params = useParams<{ key: string }>();
  const key = params?.key ?? "";
  const router = useRouter();
  const qc = useQueryClient();

  const projectQ = useQuery({ queryKey: ["project", key], queryFn: () => getProject(key) });
  const meQ = useQuery({ queryKey: ["me"], queryFn: () => me() });
  const sprintsQ = useQuery({ queryKey: ["sprints", key], queryFn: () => listSprints(key) });
  const backlogQ = useQuery({
    queryKey: ["backlog", key],
    queryFn: () => listBacklog(key),
    retry: false,
  });

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const apply = useMutation({
    mutationFn: (op: BulkOp) => bulkTasks(key, [...selected], op),
    onSuccess: () => {
      setSelected(new Set());
      setError(null);
      qc.invalidateQueries({ queryKey: ["backlog", key] });
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "bulk action failed"),
  });

  if (backlogQ.error) {
    const e = backlogQ.error as unknown as ApiError;
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
    // Without this, a failed fetch fell through to "Backlog zero" — a lie.
    return (
      <AppShell currentProjectKey={key}>
        <LoadError what="The backlog" message={e.message} onRetry={() => backlogQ.refetch()} />
      </AppShell>
    );
  }

  const canManage = projectQ.data?.your_role === "lead";
  // Task creation is open to leads and contributors (watchers/viewers can't) —
  // mirrors the API's create-task gate.
  const canCreate =
    projectQ.data?.your_role === "lead" || projectQ.data?.your_role === "contributor";
  const items = backlogQ.data ?? [];
  const sprints = (sprintsQ.data ?? []).filter((s) => s.state !== "completed");

  const allSelected = items.length > 0 && selected.size === items.length;
  const toggleAll = () =>
    setSelected(allSelected ? new Set() : new Set(items.map((i) => i.key)));
  const toggle = (k: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });

  return (
    <AppShell currentProjectKey={key}>
      <header className="mb-4 flex items-end justify-between">
        <div>
          <Breadcrumbs items={projectCrumbs(key, "backlog")} />
          <h1 className="text-3xl font-semibold">The pile.</h1>
          <p className="mt-1 text-sm text-chrome-dim">
            Tasks with no sprint. Select a few and triage them in one go.
          </p>
        </div>
      </header>

      {canManage && selected.size > 0 && (
        <div className="mono mb-3 flex flex-wrap items-center gap-2 rounded border border-accent/30 bg-accent/5 p-2 text-xs">
          <span className="text-chrome-dim">{selected.size} selected</span>
          <button
            type="button"
            onClick={() => apply.mutate({ op: "assign", assignee_id: meQ.data?.id ?? null })}
            className="inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <UserPlus size={12} /> assign to me
          </button>
          <button
            type="button"
            onClick={() => apply.mutate({ op: "assign", assignee_id: null })}
            className="inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <UserMinus size={12} /> unassign
          </button>
          <label className="inline-flex items-center gap-1 text-chrome-dim">
            move to
            <select
              aria-label="move to sprint"
              defaultValue=""
              onChange={(e) => {
                if (e.target.value) apply.mutate({ op: "sprint", sprint_id: e.target.value });
                e.target.value = "";
              }}
              className="rounded border border-white/10 bg-ink px-1.5 py-0.5 text-chrome"
            >
              <option value="">sprint…</option>
              {sprints.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
          </label>
          <button
            type="button"
            onClick={() => {
              if (confirm(`Delete ${selected.size} task(s)? Soft delete — an admin can restore.`))
                apply.mutate({ op: "delete" });
            }}
            className="inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-chrome-dim hover:border-red-500/40 hover:text-red-300"
          >
            <Trash2 size={12} /> delete
          </button>
          {error && <span className="text-red-300">{error}</span>}
        </div>
      )}

      <div className="rounded-lg border border-white/10 bg-ink-subtle">
        {canCreate && (
          <BacklogQuickAdd
            projectKey={key}
            onCreated={() => qc.invalidateQueries({ queryKey: ["backlog", key] })}
          />
        )}
        {canManage && items.length > 0 && (
          <button
            type="button"
            onClick={toggleAll}
            className="mono flex w-full items-center gap-2 border-b border-white/10 px-3 py-2 text-left text-[11px] text-chrome-dim hover:text-chrome"
          >
            {allSelected ? <CheckSquare size={13} /> : <Square size={13} />} select all
          </button>
        )}
        <ul>
          {items.map((t) => {
            const on = selected.has(t.key);
            return (
              <li
                key={t.id}
                className="flex items-center gap-2 border-b border-white/5 px-3 py-2 last:border-0"
              >
                {canManage && (
                  <button type="button" onClick={() => toggle(t.key)} aria-label={`select ${t.key}`} className="text-chrome-dim hover:text-chrome">
                    {on ? <CheckSquare size={14} className="text-accent" /> : <Square size={14} />}
                  </button>
                )}
                <span
                  className={`mono shrink-0 whitespace-nowrap rounded border px-1 py-0.5 text-[10px] uppercase ${PRIO[t.priority].cls}`}
                >
                  {PRIO[t.priority].label}
                </span>
                <Link
                  href={`/tasks/${t.key}`}
                  className="mono shrink-0 whitespace-nowrap text-xs text-accent hover:underline"
                >
                  {t.key}
                </Link>
                <span className="min-w-0 flex-1 truncate text-sm text-chrome" title={t.title}>
                  {t.title}
                </span>
                {t.assignee_id && (
                  <span className="mono ml-auto shrink-0 whitespace-nowrap text-[10px] text-chrome-dim">
                    assigned
                  </span>
                )}
              </li>
            );
          })}
          {items.length === 0 && (
            <li className="mono p-8 text-center text-sm text-chrome-dim">
              Backlog zero. Either everything&apos;s scheduled or nobody&apos;s
              filed a ticket yet.
            </li>
          )}
        </ul>
      </div>
    </AppShell>
  );
}

// Inline quick-add for the backlog. Mirrors the board's "+ add card": a title
// input where Enter files a sprint-less task in the default board's first
// column, Esc collapses. Stays open + refocuses after each add for rapid
// entry. Empty submit gets an inline nudge (QA F5), never a silent no-op.
function BacklogQuickAdd({
  projectKey,
  onCreated,
}: {
  projectKey: string;
  onCreated: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [titleError, setTitleError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const create = useMutation({
    // No column_id / sprint_id: the API slots it into the default board's first
    // column with no sprint, which is exactly a backlog task.
    mutationFn: (t: string) => createTask(projectKey, { title: t }),
    onSuccess: () => {
      onCreated();
      setTitle("");
      setError(null);
      inputRef.current?.focus();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "couldn't file it"),
  });

  function close() {
    setOpen(false);
    setTitle("");
    setTitleError(null);
    setError(null);
  }

  if (!open) {
    return (
      <button
        type="button"
        data-backlog-quick-add
        onClick={() => setOpen(true)}
        className="mono flex w-full items-center gap-1 border-b border-white/10 px-3 py-2 text-left text-xs text-chrome-dim hover:text-chrome"
      >
        <Plus size={12} /> add a task
      </button>
    );
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!title.trim()) {
          setTitleError("Needs a title.");
          return;
        }
        create.mutate(title.trim());
      }}
      className="space-y-1 border-b border-white/10 p-2"
    >
      <input
        ref={inputRef}
        autoFocus
        value={title}
        onChange={(e) => {
          setTitle(e.target.value);
          if (titleError) setTitleError(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
        placeholder="task title"
        aria-label="new task title"
        aria-invalid={!!titleError}
        className={`block w-full rounded border bg-ink px-2 py-1 text-sm text-chrome focus:outline-none placeholder:text-chrome-dim/50 placeholder:italic ${
          titleError ? "border-red-500/60 focus:border-red-500" : "border-white/10 focus:border-accent"
        }`}
      />
      {titleError && <div className="mono text-[11px] text-red-300">{titleError}</div>}
      {error && <div className="mono text-[11px] text-red-300">{error}</div>}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={close}
          className="mono text-[10px] text-chrome-dim hover:text-chrome"
        >
          :q cancel
        </button>
        <button
          type="submit"
          disabled={create.isPending}
          className="mono rounded bg-accent px-2 py-1 text-[10px] text-accent-fg disabled:opacity-50"
        >
          {create.isPending ? "…" : "add"}
        </button>
      </div>
    </form>
  );
}
