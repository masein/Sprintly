"use client";

// Sprint detail. Header (name/goal/dates/state) + actions (start/complete) +
// task assignment + burndown chart + summary (when retro is closed).

import { useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { Play, CheckCircle2, GripVertical, Plus, Trash2, X } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Breadcrumbs, projectCrumbs } from "@/components/Breadcrumbs";
import { BurndownChart } from "@/components/BurndownChart";
import { ListSearch, matchesTask } from "@/components/ListSearch";
import { LoadError } from "@/components/LoadError";
import { Markdown } from "@/components/Markdown";
import {
  assignTaskToSprint,
  completeSprint,
  getBurndown,
  getSprint,
  listSprintTasks,
  listSprints,
  startSprint,
  unassignTaskFromSprint,
  type CarryOver,
  type Sprint,
  type SprintTask,
} from "@/lib/sprints";
import { listBacklog } from "@/lib/templates";
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
  const projectKey = sprintQ.data?.project_key;
  const sprintOpen = sprintQ.data != null && sprintQ.data.state !== "completed";
  const backlogQ = useQuery({
    queryKey: ["backlog", projectKey],
    queryFn: () => listBacklog(projectKey!),
    enabled: !!projectKey && sprintOpen,
  });

  // Search over the sprint's tasks and the backlog panel at once — both lists
  // are already loaded, so this is instant. (Named taskQuery, not `search`:
  // lib/search's `search()` is already in scope here for the task typeahead.)
  const [taskQuery, setTaskQuery] = useState("");

  const invalidateLists = () => {
    qc.invalidateQueries({ queryKey: ["sprint-tasks", id] });
    qc.invalidateQueries({ queryKey: ["sprint", id] });
    qc.invalidateQueries({ queryKey: ["sprint-burndown", id] });
    qc.invalidateQueries({ queryKey: ["backlog", projectKey] });
  };
  const pullIn = useMutation({
    mutationFn: (taskKey: string) => assignTaskToSprint(id, taskKey),
    onSuccess: invalidateLists,
    onError: (e) => alert((e as unknown as ApiError).message),
  });
  const pushOut = useMutation({
    mutationFn: (taskKey: string) => unassignTaskFromSprint(id, taskKey),
    onSuccess: invalidateLists,
    onError: (e) => alert((e as unknown as ApiError).message),
  });

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );
  const onDragEnd = (ev: DragEndEvent) => {
    const overId = ev.over ? String(ev.over.id) : null;
    const [src, taskKey] = String(ev.active.id).split(":");
    if (!taskKey || !overId) return;
    if (src === "backlog" && overId === "sprint-drop") pullIn.mutate(taskKey);
    if (src === "sprint" && overId === "backlog-drop") pushOut.mutate(taskKey);
  };

  const start = useMutation({
    mutationFn: () => startSprint(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sprint", id] }),
    onError: (e) => alert((e as unknown as ApiError).message),
  });
  const [completing, setCompleting] = useState(false);
  const complete = useMutation({
    mutationFn: (carry?: CarryOver) => completeSprint(id, carry),
    onSuccess: (res) => {
      setCompleting(false);
      qc.invalidateQueries({ queryKey: ["sprint", id] });
      qc.invalidateQueries({ queryKey: ["sprints", sprintQ.data?.project_key] });
      qc.invalidateQueries({ queryKey: ["backlog", sprintQ.data?.project_key] });
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
    return (
      <AppShell>
        <LoadError what="This sprint" message={e.message} onRetry={() => sprintQ.refetch()} />
      </AppShell>
    );
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
        <Breadcrumbs
          items={[
            { label: "sprintly", href: "/" },
            { label: sprint.project_key, href: `/projects/${sprint.project_key}` },
            { label: "sprints", href: `/projects/${sprint.project_key}/sprints` },
            { label: sprint.name },
          ]}
        />
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
            onClick={() => setCompleting(true)}
            disabled={complete.isPending}
            className="mono inline-flex items-center gap-2 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
          >
            <CheckCircle2 size={14} /> complete + open retro
          </button>
        )}
      </div>

      {completing && (
        <CompleteSprintModal
          sprint={sprint}
          unfinished={(tasksQ.data ?? []).filter((t) => t.status !== "done").length}
          busy={complete.isPending}
          onCancel={() => setCompleting(false)}
          onConfirm={(carry) => complete.mutate(carry)}
        />
      )}

      <DndContext sensors={sensors} onDragEnd={onDragEnd}>
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_360px]">
          <SprintDropZone active={sprintOpen}>
            <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
              <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
                tasks ({tasksQ.data?.length ?? 0})
              </h2>
              <ListSearch
                value={taskQuery}
                onChange={setTaskQuery}
                label="search sprint tasks"
                placeholder="search this sprint + backlog…"
                className="w-full sm:w-72"
              />
            </div>
            <TaskList
              tasks={(tasksQ.data ?? []).filter((t) => matchesTask(taskQuery, t))}
              sprintId={id}
              canManage={sprint.state !== "completed"}
              draggable={sprintOpen}
            />
            {sprint.state !== "completed" && (
              <AddTaskRow sprintId={id} projectKey={sprint.project_key} onAdded={invalidateLists} />
            )}
          </SprintDropZone>
          <aside>
            {sprintOpen && (
              <BacklogPanel
                items={(backlogQ.data ?? []).filter((t) => matchesTask(taskQuery, t))}
                loading={backlogQ.isLoading}
              />
            )}
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
      </DndContext>
    </AppShell>
  );
}

// ─── Completion modal (carry-over) ───────────────────────────────────────────
// Completing a sprint used to silently strand whatever wasn't done in the
// closed sprint. Now it asks: send the leftovers to the backlog, into another
// sprint, or into a fresh one you name right here.

function CompleteSprintModal({
  sprint,
  unfinished,
  busy,
  onCancel,
  onConfirm,
}: {
  sprint: Sprint;
  unfinished: number;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (carry?: CarryOver) => void;
}) {
  type Choice = "leave" | "backlog" | "sprint" | "new_sprint";
  const [choice, setChoice] = useState<Choice>(unfinished > 0 ? "backlog" : "leave");
  const [targetId, setTargetId] = useState("");
  const [name, setName] = useState(nextSprintName(sprint.name));
  const [starts, setStarts] = useState(() => new Date().toISOString().slice(0, 10));
  const [ends, setEnds] = useState(() => {
    const d = new Date();
    d.setDate(d.getDate() + 14);
    return d.toISOString().slice(0, 10);
  });

  const sprintsQ = useQuery({
    queryKey: ["sprints", sprint.project_key],
    queryFn: () => listSprints(sprint.project_key),
  });
  const candidates = (sprintsQ.data ?? []).filter(
    (s) => s.id !== sprint.id && s.state !== "completed",
  );

  function submit(e: React.FormEvent) {
    e.preventDefault();
    if (unfinished === 0 || choice === "leave") return onConfirm(undefined);
    if (choice === "backlog") return onConfirm({ to: "backlog" });
    if (choice === "sprint") {
      if (!targetId) return;
      return onConfirm({ to: "sprint", sprint_id: targetId });
    }
    if (!name.trim()) return;
    onConfirm({
      to: "new_sprint",
      name: name.trim(),
      starts_at: new Date(`${starts}T00:00:00Z`).toISOString(),
      ends_at: new Date(`${ends}T00:00:00Z`).toISOString(),
    });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <form
        onSubmit={submit}
        data-testid="complete-sprint-modal"
        className="max-h-[90vh] w-full max-w-lg space-y-4 overflow-y-auto rounded-lg border border-white/10 bg-ink p-5"
      >
        <header className="space-y-1">
          <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
            complete {sprint.name}
          </h2>
          <p className="text-sm text-chrome">
            {unfinished === 0 ? (
              <>Everything here is done. Clean finish.</>
            ) : (
              <>
                <span className="mono text-accent">{unfinished}</span>{" "}
                {unfinished === 1 ? "task isn't" : "tasks aren't"} done yet. Where
                should {unfinished === 1 ? "it" : "they"} go?
              </>
            )}
          </p>
        </header>

        {unfinished > 0 && (
          <div className="space-y-2">
            <Radio
              name="carry"
              value="backlog"
              checked={choice === "backlog"}
              onChange={() => setChoice("backlog")}
              label="move to the backlog"
              hint="sprint-less again, ready to re-plan"
            />
            <Radio
              name="carry"
              value="sprint"
              checked={choice === "sprint"}
              onChange={() => setChoice("sprint")}
              label="move to another sprint"
              hint={candidates.length === 0 ? "no other open sprint" : undefined}
              disabled={candidates.length === 0}
            />
            {choice === "sprint" && candidates.length > 0 && (
              <select
                value={targetId}
                onChange={(e) => setTargetId(e.target.value)}
                aria-label="target sprint"
                className="mono ml-6 block w-[calc(100%-1.5rem)] rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
              >
                <option value="">pick a sprint…</option>
                {candidates.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name} · {c.state}
                  </option>
                ))}
              </select>
            )}
            <Radio
              name="carry"
              value="new_sprint"
              checked={choice === "new_sprint"}
              onChange={() => setChoice("new_sprint")}
              label="move to a new sprint"
              hint="created planned — start it when you're ready"
            />
            {choice === "new_sprint" && (
              <div className="ml-6 space-y-2">
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  aria-label="new sprint name"
                  placeholder="Sprint 24"
                  className="mono block w-full rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
                />
                <div className="flex items-center gap-2">
                  <input
                    type="date"
                    value={starts}
                    onChange={(e) => setStarts(e.target.value)}
                    aria-label="new sprint start"
                    className="mono rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
                  />
                  <span className="mono text-[10px] text-chrome-dim">→</span>
                  <input
                    type="date"
                    value={ends}
                    onChange={(e) => setEnds(e.target.value)}
                    aria-label="new sprint end"
                    className="mono rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
                  />
                </div>
              </div>
            )}
            <Radio
              name="carry"
              value="leave"
              checked={choice === "leave"}
              onChange={() => setChoice("leave")}
              label="leave them here"
              hint="they stay in this completed sprint"
            />
          </div>
        )}

        <p className="mono text-[11px] text-chrome-dim">
          Completing snapshots velocity and opens the retro. The sprint itself
          can&apos;t be re-opened.
        </p>

        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="mono text-xs text-chrome-dim hover:text-chrome"
          >
            :q
          </button>
          <button
            type="submit"
            disabled={busy || (choice === "sprint" && !targetId)}
            className="mono inline-flex items-center gap-2 rounded bg-accent px-3 py-2 text-xs font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
          >
            <CheckCircle2 size={13} /> {busy ? "completing…" : "complete sprint"}
          </button>
        </div>
      </form>
    </div>
  );
}

function Radio({
  name,
  value,
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  name: string;
  value: string;
  checked: boolean;
  onChange: () => void;
  label: string;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <label
      className={`flex items-start gap-2 text-sm ${
        disabled ? "opacity-50" : "cursor-pointer"
      }`}
    >
      <input
        type="radio"
        name={name}
        value={value}
        checked={checked}
        onChange={onChange}
        disabled={disabled}
        className="mt-1"
      />
      <span>
        <span className="text-chrome">{label}</span>
        {hint && <span className="mono block text-[10px] text-chrome-dim">{hint}</span>}
      </span>
    </label>
  );
}

/** "Sprint 12" → "Sprint 13"; anything else gets a " (next)" suffix. */
function nextSprintName(current: string): string {
  const m = current.match(/^(.*?)(\d+)\s*$/);
  if (!m) return `${current} (next)`;
  return `${m[1]}${Number(m[2]) + 1}`;
}

// ─── Sprint ↔ backlog drag (QA: "show the backlog in the sprint view") ──────
// Two drop zones, one DndContext: drag a backlog row onto the task list to
// commit it, drag a sprint row onto the backlog panel to send it back. The
// per-row buttons stay — drag is a shortcut, not the only door.

function SprintDropZone({
  active,
  children,
}: {
  active: boolean;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: "sprint-drop", disabled: !active });
  return (
    <section
      ref={setNodeRef}
      // min-w-0: grid children default to min-width auto, so one long
      // unbreakable task title widened this column and shoved the sidebar
      // off-viewport — bounding it lets the row-level truncation do its job.
      className={`min-w-0 rounded-lg transition ${
        isOver ? "bg-accent/5 ring-1 ring-accent/40" : ""
      }`}
      data-testid="sprint-drop"
    >
      {children}
    </section>
  );
}

function DragHandle({
  attributes,
  listeners,
}: {
  attributes: React.HTMLAttributes<HTMLButtonElement>;
  listeners: Record<string, unknown> | undefined;
}) {
  return (
    <button
      type="button"
      {...attributes}
      {...(listeners as React.DOMAttributes<HTMLButtonElement>)}
      className="cursor-grab touch-none text-chrome-dim hover:text-chrome active:cursor-grabbing"
      aria-label="drag to move"
    >
      <GripVertical size={12} />
    </button>
  );
}

function BacklogPanel({ items, loading }: { items: { key: string; title: string }[]; loading: boolean }) {
  const { setNodeRef, isOver } = useDroppable({ id: "backlog-drop" });
  return (
    <section
      ref={setNodeRef}
      data-testid="backlog-drop"
      className={`mb-4 rounded-lg border border-white/10 bg-ink-subtle p-3 transition ${
        isOver ? "bg-accent/5 ring-1 ring-accent/40" : ""
      }`}
    >
      <h2 className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
        backlog ({items.length}) · drag across
      </h2>
      <ul className="max-h-72 space-y-1 overflow-y-auto">
        {loading && (
          <li className="mono text-[11px] text-chrome-dim">digging the pile…</li>
        )}
        {!loading && items.length === 0 && (
          <li className="mono rounded border border-dashed border-white/10 p-3 text-center text-[11px] text-chrome-dim">
            backlog zero — nothing to pull
          </li>
        )}
        {items.map((t) => (
          <BacklogRow key={t.key} taskKey={t.key} title={t.title} />
        ))}
      </ul>
    </section>
  );
}

function BacklogRow({ taskKey, title }: { taskKey: string; title: string }) {
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `backlog:${taskKey}`,
  });
  return (
    <li
      ref={setNodeRef}
      style={{
        transform: CSS.Translate.toString(transform),
        opacity: isDragging ? 0.6 : undefined,
        zIndex: isDragging ? 30 : undefined,
        position: isDragging ? "relative" : undefined,
      }}
      className="flex items-center gap-2 rounded border border-white/10 bg-ink px-2 py-1.5"
    >
      <DragHandle attributes={attributes} listeners={listeners} />
      <Link
        href={`/tasks/${taskKey}`}
        className="mono shrink-0 text-xs text-accent hover:underline"
      >
        {taskKey}
      </Link>
      <span className="min-w-0 flex-1 truncate text-xs text-chrome" title={title}>
        {title}
      </span>
    </li>
  );
}

function SprintTaskRow({
  task,
  draggable,
  canManage,
  onUnassign,
}: {
  task: SprintTask;
  draggable: boolean;
  canManage: boolean;
  onUnassign: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `sprint:${task.key}`,
    disabled: !draggable,
  });
  return (
    <li
      ref={setNodeRef}
      style={{
        transform: CSS.Translate.toString(transform),
        opacity: isDragging ? 0.6 : undefined,
        zIndex: isDragging ? 30 : undefined,
        position: isDragging ? "relative" : undefined,
      }}
      className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2"
    >
      {draggable && <DragHandle attributes={attributes} listeners={listeners} />}
      <span className="mono shrink-0 whitespace-nowrap text-[10px] uppercase tracking-widest text-chrome-dim">
        {task.status}
      </span>
      <Link
        href={`/tasks/${task.key}`}
        className="mono shrink-0 whitespace-nowrap text-xs text-accent hover:underline"
      >
        {task.key}
      </Link>
      <span className="min-w-0 flex-1 truncate text-sm text-chrome" title={task.title}>
        {task.title}
      </span>
      <span className="mono shrink-0 whitespace-nowrap text-xs text-chrome-dim">
        {task.story_points != null ? `${task.story_points} pts` : "—"}
      </span>
      {canManage && (
        <button
          type="button"
          onClick={onUnassign}
          className="text-chrome-dim hover:text-red-300"
          aria-label="Remove from sprint"
        >
          <Trash2 size={12} />
        </button>
      )}
    </li>
  );
}

function TaskList({
  tasks,
  sprintId,
  canManage,
  draggable,
}: {
  tasks: SprintTask[];
  sprintId: string;
  canManage: boolean;
  draggable: boolean;
}) {
  const qc = useQueryClient();
  const unassign = useMutation({
    mutationFn: (key: string) => unassignTaskFromSprint(sprintId, key),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sprint-tasks", sprintId] });
      qc.invalidateQueries({ queryKey: ["sprint", sprintId] });
      qc.invalidateQueries({ queryKey: ["sprint-burndown", sprintId] });
      qc.invalidateQueries({ queryKey: ["backlog"] });
    },
  });
  if (tasks.length === 0) {
    return (
      <div className="mono rounded border border-dashed border-white/10 p-4 text-center text-xs text-chrome-dim">
        nothing in this sprint yet — drag something over from the backlog
      </div>
    );
  }
  return (
    <ul className="space-y-1">
      {tasks.map((t) => (
        <SprintTaskRow
          key={t.key}
          task={t}
          draggable={draggable}
          canManage={canManage}
          onUnassign={() => unassign.mutate(t.key)}
        />
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
