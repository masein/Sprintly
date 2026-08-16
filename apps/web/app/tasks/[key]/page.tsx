"use client";

// Task detail page. Two-column layout: main content (title, markdown body,
// comments, activity) on the left; sidebar (status/priority/type, watchers,
// attachments) on the right. Inline edit on title and description.

import { useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { Pencil, X, Check, Trash2 } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Breadcrumbs, projectCrumbs } from "@/components/Breadcrumbs";
import { Markdown } from "@/components/Markdown";
import { CommentThread } from "@/components/CommentThread";
import { MentionTextarea } from "@/components/MentionTextarea";
import { ActivityFeed } from "@/components/ActivityFeed";
import { Attachments } from "@/components/Attachments";
import { Watchers } from "@/components/Watchers";
import { SubtasksPanel, LinksPanel } from "@/components/Relations";
import { FieldValuesPanel } from "@/components/FieldValuesPanel";
import { GitLinksPanel } from "@/components/GitLinksPanel";
import { TaskTimer } from "@/components/TaskTimer";
import { Avatar } from "@/components/Avatar";
import { AssigneePicker } from "@/components/AssigneePicker";
import { deleteTask, editTask, getTask, moveTask, restoreTask, type Task } from "@/lib/tasks";
import { showToast } from "@/lib/toast";
import { listSubtasks, setTaskParent } from "@/lib/relations";
import { search } from "@/lib/search";
import { assignTaskEpic, listEpics } from "@/lib/roadmap";
import { assignTaskToSprint, listSprints, unassignTaskFromSprint } from "@/lib/sprints";
import { me } from "@/lib/auth-bundle";
import { getProject, listBoards, listMembers } from "@/lib/projects";
import { labelColorMap, listProjectLabels } from "@/lib/labels";
import type { ApiError } from "@/lib/api";

const TYPES = ["feature", "bug", "chore", "spike", "incident"] as const;
const PRIORITIES = ["p0", "p1", "p2", "p3"] as const;

export default function TaskPage() {
  const router = useRouter();
  const params = useParams<{ key: string }>();
  const taskKey = params?.key ?? "";

  const taskQ = useQuery({
    queryKey: ["task", taskKey],
    queryFn: () => getTask(taskKey),
    enabled: !!taskKey,
  });
  const projectQ = useQuery({
    queryKey: ["project", taskQ.data?.project_key],
    queryFn: () => getProject(taskQ.data!.project_key),
    enabled: !!taskQ.data?.project_key,
  });
  const meQ = useQuery({ queryKey: ["me"], queryFn: () => me() });

  if (taskQ.error) {
    const err = taskQ.error as unknown as ApiError;
    if (err.status === 401) {
      router.push("/login");
      return null;
    }
    return (
      <AppShell>
        <div className="mono rounded border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-200">
          {err.message}
        </div>
      </AppShell>
    );
  }
  if (!taskQ.data) {
    return (
      <AppShell currentProjectKey={undefined}>
        <div className="mono text-sm text-chrome-dim">git fetch --rebase your-stuff…</div>
      </AppShell>
    );
  }

  const task = taskQ.data;
  const canManage = projectQ.data?.your_role === "lead" || projectQ.data?.your_role === "contributor";
  const canDelete = projectQ.data?.your_role === "lead" || meQ.data?.role === "admin";

  return (
    <AppShell currentProjectKey={task.project_key}>
      <div className="mb-4 flex items-center gap-3">
        <Breadcrumbs
          items={[
            { label: "sprintly", href: "/" },
            { label: task.project_key, href: `/projects/${task.project_key}` },
            ...(task.parent_key
              ? [{ label: task.parent_key, href: `/tasks/${task.parent_key}` }]
              : []),
            { label: task.key },
          ]}
        />
        {canDelete && (
          <button
            type="button"
            onClick={async () => {
              const key = task.key;
              const projectKey = task.project_key;
              await deleteTask(key);
              router.push(`/projects/${projectKey}`);
              // No confirm dialog — the undo IS the safety net, and it
              // recovers faster than anyone can re-read a warning.
              showToast(`Deleted ${key}.`, {
                actionLabel: "undo",
                onAction: async () => {
                  try {
                    await restoreTask(key);
                    showToast(`${key} is back.`);
                  } catch {
                    showToast(`Couldn't restore ${key} — an admin still can.`);
                  }
                },
              });
            }}
            className="mono ml-auto flex items-center gap-1 text-xs text-chrome-dim hover:text-red-300"
          >
            <Trash2 size={12} /> delete
          </button>
        )}
      </div>

      <div className="grid grid-cols-1 gap-8 lg:grid-cols-[1fr_280px]">
        <div className="min-w-0 space-y-8">
          <Header task={task} canEdit={canManage} />
          <Description task={task} canEdit={canManage} />
          <CommentThread taskKey={task.key} projectKey={task.project_key} />
          <ActivityFeed taskKey={task.key} />
        </div>
        <aside className="space-y-6">
          <Sidebar task={task} canEdit={canManage} />
          <FieldValuesPanel taskKey={task.key} canEdit={canManage} />
          <TaskTimer taskKey={task.key} />
          <SubtasksPanel
            parentTaskKey={task.key}
            projectKey={task.project_key}
            projectId={task.project_id}
            canManage={canManage}
          />
          <LinksPanel taskKey={task.key} canManage={canManage} />
          <GitLinksPanel taskKey={task.key} />
          <Watchers taskKey={task.key} />
          <Attachments taskKey={task.key} canManage={canManage} />
        </aside>
      </div>
    </AppShell>
  );
}

// ─── Header (title) ─────────────────────────────────────────────────────────

function Header({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(task.title);
  const save = useMutation({
    mutationFn: () => editTask(task.key, { title }),
    onSuccess: () => {
      setEditing(false);
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
    },
  });

  if (editing) {
    return (
      <form
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
        className="flex min-w-0 items-center gap-2"
      >
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          className="min-w-0 flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-2xl font-semibold text-chrome focus:border-accent focus:outline-none"
        />
        <button type="submit" className="mono shrink-0 text-xs text-accent">save</button>
        <button
          type="button"
          onClick={() => { setEditing(false); setTitle(task.title); }}
          className="mono shrink-0 text-xs text-chrome-dim"
        >cancel</button>
      </form>
    );
  }
  return (
    <header className="flex items-start gap-2">
      <h1 className="text-2xl font-semibold leading-tight">{task.title}</h1>
      {canEdit && (
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="mt-1 text-chrome-dim hover:text-chrome"
          aria-label="Rename"
        >
          <Pencil size={14} />
        </button>
      )}
    </header>
  );
}

// ─── Description (markdown) ─────────────────────────────────────────────────

function Description({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [body, setBody] = useState(task.description);

  const save = useMutation({
    mutationFn: () => editTask(task.key, { description: body }),
    onSuccess: () => {
      setEditing(false);
      qc.invalidateQueries({ queryKey: ["task", task.key] });
    },
  });

  if (editing) {
    return (
      <section className="space-y-2">
        <MentionTextarea
          autoFocus
          value={body}
          onChange={setBody}
          projectKey={task.project_key}
          rows={8}
          className="block w-full rounded border border-white/10 bg-ink-subtle px-3 py-2 text-sm text-chrome focus:border-accent focus:outline-none"
          placeholder="markdown — backticks for `code`, **bold**, @handle to mention, etc."
        />
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={() => { setEditing(false); setBody(task.description); }}
            className="mono text-xs text-chrome-dim hover:text-chrome"
          >
            <X size={11} className="-mt-0.5 mr-1 inline" />:q
          </button>
          <button
            type="button"
            onClick={() => save.mutate()}
            className="mono inline-flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-xs text-accent-fg"
          >
            <Check size={11} /> :wq
          </button>
        </div>
      </section>
    );
  }

  return (
    <section className="group rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mb-1 flex items-center justify-between">
        <span className="mono text-xs uppercase tracking-widest text-chrome-dim">
          description
        </span>
        {canEdit && (
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="mono text-xs text-chrome-dim opacity-0 transition group-hover:opacity-100 hover:text-chrome"
          >
            <Pencil size={11} className="-mt-0.5 mr-1 inline" />edit
          </button>
        )}
      </div>
      {task.description ? (
        <Markdown>{task.description}</Markdown>
      ) : (
        <p className="mono text-xs text-chrome-dim">no description yet</p>
      )}
    </section>
  );
}

// ─── Sidebar (status/priority/type/labels) ──────────────────────────────────

function Sidebar({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const patch = useMutation({
    mutationFn: (p: Partial<Task>) => editTask(task.key, p),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
    },
  });

  return (
    <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
        details
      </h2>
      <ParentField task={task} canEdit={canEdit} />
      <StatusField task={task} canEdit={canEdit} />
      <Field
        label="priority"
        value={task.priority}
        options={canEdit ? PRIORITIES.slice() : undefined}
        onChange={(v) => patch.mutate({ priority: v as Task["priority"] })}
      />
      <Field
        label="type"
        value={task.type}
        options={canEdit ? TYPES.slice() : undefined}
        onChange={(v) => patch.mutate({ type: v as Task["type"] })}
      />
      <AssigneeField task={task} canEdit={canEdit} />
      <SprintField task={task} canEdit={canEdit} />
      <EpicField task={task} canEdit={canEdit} />
      <PlanningFields task={task} canEdit={canEdit} />
      <LabelsField task={task} canEdit={canEdit} />
    </section>
  );
}

// ─── Planning fields (points / due / estimate) ───────────────────────────────
// The schema and API have carried story_points, due_date, and
// estimate_minutes since day one — the sidebar just never offered a way to
// set them (they only showed read-only once an import filled them in).

function PlanningFields({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const patch = useMutation({
    mutationFn: (p: Partial<Task>) => editTask(task.key, p),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
    },
    onError: (e) => alert((e as unknown as ApiError).message),
  });

  // Same key the subtasks panel uses, so this is a cache read, not a second
  // request. The parent's estimate doesn't silently absorb its children's —
  // that would overwrite a number someone typed — but the roll-up is shown
  // right under it, which is what "the parent should know" actually needs.
  const subs = useQuery({
    queryKey: ["subtasks", task.key],
    queryFn: () => listSubtasks(task.key),
    staleTime: 30_000,
  });
  const subtaskEstimate = (subs.data ?? []).reduce(
    (sum, s) => sum + (s.estimate_minutes ?? 0),
    0,
  );
  const rollup =
    subtaskEstimate > 0 ? (
      <div className="flex items-center justify-between gap-3">
        <span
          className="mono text-[10px] uppercase tracking-widest text-chrome-dim"
          title="own estimate + subtask estimates"
        >
          Σ w/ subtasks
        </span>
        <span className="mono text-xs text-chrome">
          {fmtEstimate((task.estimate_minutes ?? 0) + subtaskEstimate)}
        </span>
      </div>
    ) : null;

  if (!canEdit) {
    return (
      <>
        {task.story_points != null && (
          <Field label="points" value={String(task.story_points)} />
        )}
        {task.due_date && <Field label="due" value={task.due_date} />}
        {task.estimate_minutes != null && (
          <Field label="estimate" value={fmtEstimate(task.estimate_minutes)} />
        )}
        {rollup}
      </>
    );
  }

  return (
    <>
      <div className="flex items-center justify-between gap-3">
        <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
          points
        </span>
        <input
          type="number"
          min={0}
          max={999}
          aria-label="story points"
          defaultValue={task.story_points ?? ""}
          onBlur={(e) => {
            const v = e.target.value === "" ? null : Number(e.target.value);
            if (v !== task.story_points) patch.mutate({ story_points: v });
          }}
          placeholder="—"
          className="mono w-20 rounded border border-white/10 bg-ink px-1.5 py-0.5 text-right text-xs text-chrome"
        />
      </div>
      <div className="flex items-center justify-between gap-3">
        <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
          due
        </span>
        <input
          type="date"
          aria-label="due date"
          defaultValue={task.due_date ?? ""}
          onChange={(e) => patch.mutate({ due_date: e.target.value || null })}
          className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome"
        />
      </div>
      <div className="flex items-center justify-between gap-3">
        <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
          estimate (h)
        </span>
        <input
          type="number"
          min={0}
          step={0.5}
          aria-label="estimate hours"
          defaultValue={task.estimate_minutes != null ? task.estimate_minutes / 60 : ""}
          onBlur={(e) => {
            const mins =
              e.target.value === "" ? null : Math.round(Number(e.target.value) * 60);
            if (mins !== task.estimate_minutes) patch.mutate({ estimate_minutes: mins });
          }}
          placeholder="—"
          className="mono w-20 rounded border border-white/10 bg-ink px-1.5 py-0.5 text-right text-xs text-chrome"
        />
      </div>
      {rollup}
    </>
  );
}

function fmtEstimate(mins: number): string {
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h === 0) return `${m}m`;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

// Status picker (QA F6): a dropdown of the board's real columns. Choosing one
// moves the card to that column, which is what sets `status` server-side — so
// the board and the detail panel can never disagree. Reuses the move endpoint;
// no new API.
function StatusField({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const boardsQ = useQuery({
    queryKey: ["boards", task.project_key],
    queryFn: () => listBoards(task.project_key),
    staleTime: 60_000,
    retry: false,
  });
  const move = useMutation({
    mutationFn: (column_id: string) => moveTask(task.key, { column_id }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
    },
  });

  const board = (boardsQ.data ?? []).find((b) => b.id === task.board_id);
  const columns = board?.columns ?? [];
  const current = columns.find((c) => c.id === task.column_id);

  // Until the columns load (or if we can't edit), show the static status text.
  if (!canEdit || columns.length === 0) {
    return <Field label="status" value={current?.name ?? task.status} />;
  }
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">status</span>
      <select
        value={task.column_id}
        onChange={(e) => move.mutate(e.target.value)}
        aria-label="status"
        disabled={move.isPending}
        className="mono max-w-[60%] truncate rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome disabled:opacity-50"
      >
        {columns.map((c) => (
          <option key={c.id} value={c.id}>
            {/* Show the category when the name doesn't already say it —
                a "Verified" column that secretly counts as todo is exactly
                the kind of thing that should be visible right here. */}
            {c.name.toLowerCase().includes(c.category.replace("_", " ")) ||
            c.name.toLowerCase() === c.category
              ? c.name
              : `${c.name} · ${c.category.replace("_", " ")}`}
          </option>
        ))}
      </select>
    </div>
  );
}

// Assignee picker (QA F2): any project member, plus "unassigned". Setting it
// reuses the task PATCH → the F5 assignment notification fires server-side.
// ─── Parent (task ↔ subtask conversion) ──────────────────────────────────────
// A top-level task can become a subtask ("make subtask of…"), a subtask can be
// promoted back ("↑ promote") or moved under a different parent — the server
// keeps the hierarchy one level deep and refuses demoting a task that has
// subtasks of its own.

function ParentField({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const [picking, setPicking] = useState(false);
  const [q, setQ] = useState("");
  const hitsQ = useQuery({
    queryKey: ["parent-search", task.key, q],
    queryFn: () => search(q, 8),
    enabled: picking && q.trim().length > 0,
  });
  const set = useMutation({
    mutationFn: (parentKey: string | null) => setTaskParent(task.key, parentKey),
    onSuccess: () => {
      setPicking(false);
      setQ("");
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
      qc.invalidateQueries({ queryKey: ["subtasks"] });
    },
    onError: (e) => alert((e as unknown as ApiError).message),
  });

  if (!canEdit && !task.parent_key) return null;

  const candidates = (hitsQ.data?.tasks ?? []).filter(
    (t) => t.project_key === task.project_key && t.key !== task.key,
  );

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between gap-3">
        <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
          parent
        </span>
        <span className="flex items-center gap-2">
          {task.parent_key ? (
            <>
              <Link
                href={`/tasks/${task.parent_key}`}
                className="mono text-xs text-accent hover:underline"
              >
                ↳ {task.parent_key}
              </Link>
              {canEdit && (
                <>
                  <button
                    type="button"
                    onClick={() => set.mutate(null)}
                    disabled={set.isPending}
                    className="mono text-[11px] text-chrome-dim hover:text-chrome disabled:opacity-50"
                    title="back to a top-level task"
                  >
                    ↑ promote
                  </button>
                  <button
                    type="button"
                    onClick={() => setPicking((v) => !v)}
                    className="mono text-[11px] text-chrome-dim hover:text-chrome"
                  >
                    move…
                  </button>
                </>
              )}
            </>
          ) : canEdit ? (
            <button
              type="button"
              onClick={() => setPicking((v) => !v)}
              className="mono text-[11px] text-chrome-dim hover:text-chrome"
            >
              make subtask of…
            </button>
          ) : null}
        </span>
      </div>
      {picking && (
        <div className="space-y-1">
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setPicking(false);
                setQ("");
              }
            }}
            placeholder="search a task in this project…"
            aria-label="parent task search"
            className="block w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          {q.trim().length > 0 && (
            <ul className="max-h-40 space-y-0.5 overflow-y-auto">
              {candidates.length === 0 && !hitsQ.isLoading && (
                <li className="mono px-1 text-[11px] text-chrome-dim">
                  no matching task here
                </li>
              )}
              {candidates.map((t) => (
                <li key={t.key}>
                  <button
                    type="button"
                    onClick={() => set.mutate(t.key)}
                    disabled={set.isPending}
                    className="mono flex w-full items-center gap-2 rounded px-1 py-0.5 text-left text-xs text-chrome hover:bg-white/5 disabled:opacity-50"
                  >
                    <span className="text-accent">{t.key}</span>
                    <span className="truncate text-chrome-dim">{t.title}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function AssigneeField({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const membersQ = useQuery({
    queryKey: ["project-members", task.project_key],
    queryFn: () => listMembers(task.project_key),
    retry: false,
  });
  const patch = useMutation({
    mutationFn: (assignee_id: string | null) => editTask(task.key, { assignee_id }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
    },
  });
  const members = membersQ.data ?? [];
  const current = members.find((m) => m.user_id === task.assignee_id);

  const currentAvatar = current ? (
    <Avatar
      size={18}
      user={{
        userId: current.user_id,
        displayName: current.display_name,
        handle: current.handle,
        avatarUrl: current.avatar_url,
        avatarStyle: current.avatar_style,
        avatarSeed: current.avatar_seed,
      }}
    />
  ) : null;

  if (!canEdit) {
    if (!current) return null;
    return (
      <div className="flex items-center justify-between gap-3">
        <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">assignee</span>
        <span className="mono flex items-center gap-1.5 text-xs text-chrome">
          {currentAvatar}@{current.handle}
        </span>
      </div>
    );
  }
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">assignee</span>
      <div className="flex items-center gap-1.5">
        {currentAvatar}
        <AssigneePicker
          members={members}
          value={task.assignee_id}
          onChange={(id) => patch.mutate(id)}
          disabled={patch.isPending}
        />
      </div>
    </div>
  );
}

// Labels multi-select (QA F3): chips for what's on the task (colour + text, with
// an × to remove) and a picker of the project palette to add. Persists via the
// task PATCH labels array.
function LabelsField({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const paletteQ = useQuery({
    queryKey: ["project-labels", task.project_key],
    queryFn: () => listProjectLabels(task.project_key),
    retry: false,
    staleTime: 60_000,
  });
  const patch = useMutation({
    mutationFn: (labels: string[]) => editTask(task.key, { labels }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
    },
  });
  const palette = paletteQ.data ?? [];
  const colors = labelColorMap(palette);
  const current = task.labels;
  const available = palette.filter(
    (l) => !current.some((c) => c.toLowerCase() === l.name.toLowerCase()),
  );

  if (current.length === 0 && (!canEdit || available.length === 0)) return null;

  return (
    <div>
      <span className="mono block text-[10px] uppercase tracking-widest text-chrome-dim">
        labels
      </span>
      <div className="mt-1 flex flex-wrap items-center gap-1">
        {current.map((l) => {
          const c = colors[l.toLowerCase()];
          return (
            <span
              key={l}
              className="mono inline-flex items-center gap-1 rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim"
              style={c ? { borderColor: `${c}66`, color: c, background: `${c}14` } : undefined}
            >
              {l}
              {canEdit && (
                <button
                  type="button"
                  aria-label={`remove ${l}`}
                  onClick={() => patch.mutate(current.filter((x) => x !== l))}
                  className="text-current opacity-70 hover:opacity-100"
                >
                  <X size={9} />
                </button>
              )}
            </span>
          );
        })}
        {canEdit && available.length > 0 && (
          <select
            value=""
            onChange={(e) => {
              if (e.target.value) patch.mutate([...current, e.target.value]);
            }}
            aria-label="add label"
            className="mono rounded border border-dashed border-white/15 bg-ink px-1 py-0.5 text-[10px] text-chrome-dim"
          >
            <option value="">+ label</option>
            {available.map((l) => (
              <option key={l.id} value={l.name}>
                {l.name}
              </option>
            ))}
          </select>
        )}
        {current.length === 0 && (!canEdit || available.length === 0) && (
          <span className="mono text-[10px] text-chrome-dim">none</span>
        )}
      </div>
    </div>
  );
}

// Which sprint the task lives in — or the backlog. This is the discoverable
// way OUT of a sprint (the sprint page's remove icon being the only other
// one). Uses the sprint assign/unassign endpoints; assigning to a different
// sprint replaces the current one server-side.
function SprintField({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const sprintsQ = useQuery({
    queryKey: ["sprints", task.project_key],
    queryFn: () => listSprints(task.project_key),
    retry: false,
  });
  const change = useMutation({
    mutationFn: async (next: string) => {
      if (next === "") {
        // "backlog" — drop out of the current sprint (no-op if none).
        if (task.sprint_id) await unassignTaskFromSprint(task.sprint_id, task.key);
        return;
      }
      await assignTaskToSprint(next, task.key);
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["tasks", task.project_id] });
      qc.invalidateQueries({ queryKey: ["sprints", task.project_key] });
    },
  });

  const sprints = sprintsQ.data ?? [];
  const current = sprints.find((s) => s.id === task.sprint_id);
  // Assignable targets: planned + active. The current sprint stays listed even
  // if completed, so the select always shows the truth.
  const options = sprints.filter(
    (s) => s.state !== "completed" || s.id === task.sprint_id,
  );

  if (!canEdit) {
    return current ? <Field label="sprint" value={current.name} /> : null;
  }
  if (sprints.length === 0) return null; // no sprints yet — nothing to pick

  return (
    <div className="flex items-center justify-between gap-3">
      <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">sprint</span>
      <select
        value={task.sprint_id ?? ""}
        onChange={(e) => change.mutate(e.target.value)}
        aria-label="sprint"
        disabled={change.isPending}
        className="mono max-w-[60%] truncate rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome disabled:opacity-50"
      >
        <option value="">backlog · none</option>
        {options.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
            {s.state === "active" ? " · active" : s.state === "completed" ? " · completed" : ""}
          </option>
        ))}
      </select>
    </div>
  );
}

// The task's epic (F6). Read-only label for viewers; a select for editors.
function EpicField({ task, canEdit }: { task: Task; canEdit: boolean }) {
  const qc = useQueryClient();
  const epicsQ = useQuery({
    queryKey: ["epics", task.project_key],
    queryFn: () => listEpics(task.project_key),
    retry: false,
  });
  const assign = useMutation({
    mutationFn: (epicId: string | null) => assignTaskEpic(task.key, epicId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["task", task.key] });
      qc.invalidateQueries({ queryKey: ["epics", task.project_key] });
    },
  });
  const epics = epicsQ.data ?? [];
  const current = epics.find((e) => e.id === task.epic_id);

  if (!canEdit) {
    return current ? <Field label="epic" value={current.name} /> : null;
  }
  if (epics.length === 0) return null; // nothing to assign to yet

  return (
    <div className="flex items-center justify-between gap-3">
      <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">epic</span>
      <select
        value={task.epic_id ?? ""}
        onChange={(e) => assign.mutate(e.target.value || null)}
        aria-label="epic"
        className="mono max-w-[60%] truncate rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome"
      >
        <option value="">none</option>
        {epics.map((e) => (
          <option key={e.id} value={e.id}>{e.name}</option>
        ))}
      </select>
    </div>
  );
}

function Field({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options?: string[];
  onChange?: (v: string) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
        {label}
      </span>
      {options ? (
        <select
          value={value}
          onChange={(e) => onChange?.(e.target.value)}
          className="mono max-w-[60%] truncate rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome"
        >
          {options.map((o) => (
            <option key={o} value={o}>{o}</option>
          ))}
        </select>
      ) : (
        // Long values (epic names, especially) must clip inside the 280px
        // sidebar instead of pushing through its border.
        <span className="mono min-w-0 truncate text-xs text-chrome" title={value}>
          {value}
        </span>
      )}
    </div>
  );
}
