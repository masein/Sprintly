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
import { Markdown } from "@/components/Markdown";
import { CommentThread } from "@/components/CommentThread";
import { ActivityFeed } from "@/components/ActivityFeed";
import { Attachments } from "@/components/Attachments";
import { Watchers } from "@/components/Watchers";
import { SubtasksPanel, LinksPanel } from "@/components/Relations";
import { FieldValuesPanel } from "@/components/FieldValuesPanel";
import { GitLinksPanel } from "@/components/GitLinksPanel";
import { TaskTimer } from "@/components/TaskTimer";
import { deleteTask, editTask, getTask, type Task } from "@/lib/tasks";
import { assignTaskEpic, listEpics } from "@/lib/roadmap";
import { me } from "@/lib/auth-bundle";
import { getProject } from "@/lib/projects";
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
        <Link
          href={`/projects/${task.project_key}`}
          className="mono text-xs text-chrome-dim hover:text-chrome"
        >
          ← {task.project_key}
        </Link>
        <span className="mono text-xs text-chrome-dim">/</span>
        <span className="mono text-xs text-accent">{task.key}</span>
        {canDelete && (
          <button
            type="button"
            onClick={async () => {
              if (!confirm("Delete this task? Soft delete — recoverable by an admin.")) return;
              await deleteTask(task.key);
              router.push(`/projects/${task.project_key}`);
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
          <CommentThread taskKey={task.key} />
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
        className="flex items-center gap-2"
      >
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          className="w-full rounded border border-white/10 bg-ink px-2 py-1 text-2xl font-semibold text-chrome focus:border-accent focus:outline-none"
        />
        <button type="submit" className="mono text-xs text-accent">save</button>
        <button
          type="button"
          onClick={() => { setEditing(false); setTitle(task.title); }}
          className="mono text-xs text-chrome-dim"
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
        <textarea
          autoFocus
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={8}
          className="block w-full rounded border border-white/10 bg-ink-subtle px-3 py-2 text-sm text-chrome focus:border-accent focus:outline-none"
          placeholder="markdown — backticks for `code`, **bold**, * lists, etc."
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
      <Field label="status" value={task.status} />
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
      <EpicField task={task} canEdit={canEdit} />
      {task.due_date && <Field label="due" value={task.due_date} />}
      {task.estimate_minutes != null && (
        <Field label="estimate" value={`${task.estimate_minutes} min`} />
      )}
      {task.labels.length > 0 && (
        <div>
          <span className="mono block text-[10px] uppercase tracking-widest text-chrome-dim">
            labels
          </span>
          <div className="mt-1 flex flex-wrap gap-1">
            {task.labels.map((l) => (
              <span
                key={l}
                className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim"
              >
                {l}
              </span>
            ))}
          </div>
        </div>
      )}
    </section>
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
        className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome"
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
          className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome"
        >
          {options.map((o) => (
            <option key={o} value={o}>{o}</option>
          ))}
        </select>
      ) : (
        <span className="mono text-xs text-chrome">{value}</span>
      )}
    </div>
  );
}
