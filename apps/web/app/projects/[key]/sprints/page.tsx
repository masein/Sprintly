"use client";

// Per-project sprint list. Active first, then planned, then completed history.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { Plus, X } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { createSprint, listSprints, type Sprint } from "@/lib/sprints";
import type { ApiError } from "@/lib/api";

export default function SprintsPage() {
  const router = useRouter();
  const params = useParams<{ key: string }>();
  const projectKey = params?.key ?? "";

  const q = useQuery({
    queryKey: ["sprints", projectKey],
    queryFn: () => listSprints(projectKey),
    retry: (n, e) => (e as unknown as ApiError)?.status !== 401 && n < 1,
  });
  const [creating, setCreating] = useState(false);

  if (q.error) {
    const e = q.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
  }

  const items = q.data ?? [];
  return (
    <AppShell currentProjectKey={projectKey}>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · {projectKey} · sprints
          </div>
          <h1 className="text-3xl font-semibold">Sprints.</h1>
        </div>
        <button
          type="button"
          onClick={() => setCreating(true)}
          className="mono inline-flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-sm font-medium text-accent-fg hover:opacity-90"
        >
          <Plus size={14} /> new sprint
        </button>
      </header>

      {creating && (
        <CreateSprintForm
          projectKey={projectKey}
          onClose={() => setCreating(false)}
          onCreated={(id) => router.push(`/sprints/${id}`)}
        />
      )}

      {q.isLoading && (
        <div className="mono text-sm text-chrome-dim">git fetch --rebase your-stuff…</div>
      )}

      {!q.isLoading && items.length === 0 && (
        <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
          <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
            no sprints yet
          </div>
          <p className="text-chrome-dim">
            A sprint is just a named window with a goal. Start with the next 2 weeks.
          </p>
        </div>
      )}

      <ul className="space-y-2">
        {items.map((s) => (
          <li key={s.id}>
            <SprintRow sprint={s} />
          </li>
        ))}
      </ul>
    </AppShell>
  );
}

function SprintRow({ sprint }: { sprint: Sprint }) {
  return (
    <Link
      href={`/sprints/${sprint.id}`}
      className="flex items-center gap-4 rounded border border-white/10 bg-ink-subtle px-4 py-3 transition hover:border-white/20"
    >
      <span
        className={`mono inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${
          sprint.state === "active"
            ? "border-accent bg-accent/10 text-accent"
            : sprint.state === "planned"
              ? "border-white/10 text-chrome-dim"
              : "border-white/10 text-chrome-dim opacity-70"
        }`}
      >
        {sprint.state}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm text-chrome">{sprint.name}</div>
        {sprint.goal && (
          <div className="mono truncate text-[11px] text-chrome-dim">
            {sprint.goal}
          </div>
        )}
      </div>
      <div className="mono text-[10px] text-chrome-dim">
        {sprint.starts_at.slice(0, 10)} → {sprint.ends_at.slice(0, 10)}
      </div>
      <div className="mono text-xs text-chrome-dim">
        {sprint.task_count} tasks · {sprint.done_points}/{sprint.total_points} pts
      </div>
      {sprint.state === "completed" && sprint.velocity_points != null && (
        <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase text-chrome-dim">
          velocity {sprint.velocity_points}
        </span>
      )}
    </Link>
  );
}

function CreateSprintForm({
  projectKey,
  onClose,
  onCreated,
}: {
  projectKey: string;
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [goal, setGoal] = useState("");
  const today = new Date().toISOString().slice(0, 10);
  const twoWeeks = new Date(Date.now() + 14 * 86_400_000).toISOString().slice(0, 10);
  const [start, setStart] = useState(today);
  const [end, setEnd] = useState(twoWeeks);
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: () =>
      createSprint(projectKey, {
        name,
        goal: goal || undefined,
        starts_at: new Date(`${start}T00:00:00Z`).toISOString(),
        ends_at: new Date(`${end}T23:59:59Z`).toISOString(),
      }),
    onSuccess: (s) => {
      qc.invalidateQueries({ queryKey: ["sprints", projectKey] });
      onCreated(s.id);
    },
    onError: (e) => setError((e as unknown as ApiError).message),
  });

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim()) create.mutate();
      }}
      className="mb-6 space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4"
    >
      <div className="flex items-center justify-between">
        <span className="mono text-xs uppercase tracking-widest text-chrome-dim">
          new sprint
        </span>
        <button
          type="button"
          onClick={onClose}
          className="text-chrome-dim hover:text-chrome"
          aria-label="Close"
        >
          <X size={14} />
        </button>
      </div>
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        required
        placeholder="sprint name (e.g. Sprint 23)"
        className="block w-full rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <textarea
        value={goal}
        onChange={(e) => setGoal(e.target.value)}
        rows={2}
        placeholder="goal (markdown — what does success look like?)"
        className="block w-full rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <div className="flex items-center gap-2">
        <input
          type="date"
          value={start}
          onChange={(e) => setStart(e.target.value)}
          className="mono rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome"
        />
        <span className="mono text-xs text-chrome-dim">→</span>
        <input
          type="date"
          value={end}
          onChange={(e) => setEnd(e.target.value)}
          className="mono rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome"
        />
        <button
          type="submit"
          disabled={!name.trim() || create.isPending}
          className="mono ml-auto rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50"
        >
          {create.isPending ? "creating…" : "$ git init sprint"}
        </button>
      </div>
      {error && <div className="mono text-xs text-red-200">{error}</div>}
    </form>
  );
}
