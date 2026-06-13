"use client";

// Manage a project's task templates (F9): create reusable skeletons, set a
// recurrence so the worker spawns them on a cadence, spin up a task from one
// on demand, or delete. Opened from the project header.

import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FilePlus2, Repeat, Trash2, X } from "lucide-react";
import {
  createTemplate,
  deleteTemplate,
  instantiateTemplate,
  listTemplates,
  type Priority,
  type Recurrence,
  type TaskType,
} from "@/lib/templates";
import type { ApiError } from "@/lib/api";

const TYPES: TaskType[] = ["feature", "bug", "chore", "spike", "incident"];
const PRIORITIES: Priority[] = ["p0", "p1", "p2", "p3"];
const RECURRENCES: Recurrence[] = ["none", "daily", "weekly", "monthly"];

export function TemplatesManager({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const router = useRouter();
  const q = useQuery({
    queryKey: ["templates", projectKey],
    queryFn: () => listTemplates(projectKey),
    retry: false,
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: ["templates", projectKey] });

  const [name, setName] = useState("");
  const [title, setTitle] = useState("");
  const [type, setType] = useState<TaskType>("feature");
  const [priority, setPriority] = useState<Priority>("p2");
  const [recurrence, setRecurrence] = useState<Recurrence>("none");
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () =>
      createTemplate(projectKey, {
        name: name.trim(),
        title: title.trim(),
        type,
        priority,
        recurrence,
      }),
    onSuccess: () => {
      setName("");
      setTitle("");
      setRecurrence("none");
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const remove = useMutation({ mutationFn: (id: string) => deleteTemplate(id), onSuccess: invalidate });
  const spawn = useMutation({
    mutationFn: (id: string) => instantiateTemplate(id),
    onSuccess: (res) => router.push(`/tasks/${res.key}`),
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });

  const templates = q.data ?? [];

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-lg space-y-4 rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              {projectKey} · templates
            </div>
            <h2 className="text-xl font-semibold">Task templates</h2>
            <p className="mt-1 text-xs text-chrome-dim">
              Skeletons you can spawn on demand. Give one a cadence and the
              worker drops a fresh task each interval.
            </p>
          </div>
          <button type="button" onClick={onClose} className="text-chrome-dim hover:text-chrome" aria-label="Close">
            <X size={18} />
          </button>
        </div>

        <ul className="space-y-1">
          {templates.map((t) => (
            <li key={t.id} className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5">
              <span className="mono truncate text-xs text-chrome">{t.name}</span>
              <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim">
                {t.priority} · {t.type}
              </span>
              {t.recurrence !== "none" && (
                <span className="mono inline-flex items-center gap-1 rounded border border-accent/40 px-1.5 py-0.5 text-[10px] text-accent">
                  <Repeat size={9} /> {t.recurrence}
                </span>
              )}
              <span className="ml-auto flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => spawn.mutate(t.id)}
                  title="create a task from this template"
                  className="mono inline-flex items-center gap-1 rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-chrome-dim hover:border-white/20 hover:text-chrome"
                >
                  <FilePlus2 size={11} /> new task
                </button>
                <button
                  type="button"
                  aria-label={`delete ${t.name}`}
                  onClick={() => {
                    if (confirm(`Delete the "${t.name}" template?`)) remove.mutate(t.id);
                  }}
                  className="text-chrome-dim hover:text-red-300"
                >
                  <Trash2 size={13} />
                </button>
              </span>
            </li>
          ))}
          {templates.length === 0 && (
            <li className="mono text-[11px] text-chrome-dim">no templates yet — automate the boring part</li>
          )}
        </ul>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim() && title.trim()) add.mutate();
          }}
          className="space-y-2 border-t border-white/10 pt-3"
        >
          <div className="flex items-center gap-2">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={120}
              placeholder="template name"
              aria-label="template name"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <button
              type="submit"
              disabled={add.isPending || !name.trim() || !title.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              add
            </button>
          </div>
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            maxLength={120}
            placeholder="task title to prefill"
            aria-label="task title"
            className="mono w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          <div className="flex flex-wrap items-center gap-2">
            <select value={type} onChange={(e) => setType(e.target.value as TaskType)} aria-label="type" className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome">
              {TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
            </select>
            <select value={priority} onChange={(e) => setPriority(e.target.value as Priority)} aria-label="priority" className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome">
              {PRIORITIES.map((p) => <option key={p} value={p}>{p}</option>)}
            </select>
            <label className="mono flex items-center gap-1 text-[11px] text-chrome-dim">
              <Repeat size={11} /> repeat
              <select value={recurrence} onChange={(e) => setRecurrence(e.target.value as Recurrence)} aria-label="recurrence" className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome">
                {RECURRENCES.map((r) => <option key={r} value={r}>{r === "none" ? "off" : r}</option>)}
              </select>
            </label>
          </div>
          {error && <div className="mono text-[11px] text-red-300">{error}</div>}
        </form>
      </div>
    </div>
  );
}
