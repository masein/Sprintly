"use client";

// F9 — Backlog: unscheduled (no-sprint) tasks with multi-select bulk actions
// (assign, move to a sprint, delete). The board is for flow; this is for
// triaging the pile.

import { useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { CheckSquare, Square, Trash2, UserPlus, UserMinus } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { getProject } from "@/lib/projects";
import { listSprints } from "@/lib/sprints";
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
  }

  const canManage = projectQ.data?.your_role === "lead";
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
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            {key} · backlog
          </div>
          <h1 className="text-3xl font-semibold">The pile.</h1>
          <p className="mt-1 text-sm text-chrome-dim">
            Tasks with no sprint. Select a few and triage them in one go.
          </p>
        </div>
        <Link href={`/projects/${key}`} className="mono text-xs text-accent hover:underline">
          ← board
        </Link>
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
                  className={`mono rounded border px-1 py-0.5 text-[10px] uppercase ${PRIO[t.priority].cls}`}
                >
                  {PRIO[t.priority].label}
                </span>
                <Link href={`/tasks/${t.key}`} className="mono text-xs text-accent hover:underline">
                  {t.key}
                </Link>
                <span className="truncate text-sm text-chrome">{t.title}</span>
                {t.assignee_id && (
                  <span className="mono ml-auto text-[10px] text-chrome-dim">assigned</span>
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
