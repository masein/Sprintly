"use client";

// Two sidebar panels for the task detail page:
//   • Subtasks — children of this task (parent_task_id = this.id).
//   • Links    — directed edges to/from other tasks, grouped by kind.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { Plus, Trash2, ListTree, Link as LinkIcon, X } from "lucide-react";
import {
  addLink, listLinks, listSubtasks, removeLink, type LinkKind,
} from "@/lib/relations";
import { createTask, getTask } from "@/lib/tasks";
import { search } from "@/lib/search";

const KIND_LABEL: Record<LinkKind, string> = {
  blocks: "blocks",
  relates_to: "relates to",
  duplicates: "duplicates",
  parent_of: "parent of",
};
const INCOMING_LABEL: Record<LinkKind, string> = {
  blocks: "blocked by",
  relates_to: "relates to",
  duplicates: "duplicated by",
  parent_of: "child of",
};

// ─── Subtasks ───────────────────────────────────────────────────────────────

export function SubtasksPanel({
  parentTaskKey,
  projectKey,
  projectId,
  canManage,
}: {
  parentTaskKey: string;
  projectKey: string;
  projectId: string;
  canManage: boolean;
}) {
  const qc = useQueryClient();
  const subs = useQuery({
    queryKey: ["subtasks", parentTaskKey],
    queryFn: () => listSubtasks(parentTaskKey),
  });

  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const add = useMutation({
    mutationFn: async () => {
      // We pass parent_task_id via a fresh task create. We need the parent's
      // id; the task detail page has it in cache, but reading from the API
      // is one round-trip and avoids a stale-cache footgun.
      const parent = await getTask(parentTaskKey);
      await createTask(projectKey, { title, parent_task_id: parent.id });
    },
    onSuccess: () => {
      setTitle("");
      setAdding(false);
      qc.invalidateQueries({ queryKey: ["subtasks", parentTaskKey] });
      qc.invalidateQueries({ queryKey: ["tasks", projectId] });
    },
  });

  return (
    <section className="space-y-2">
      <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        <ListTree size={11} /> subtasks ({subs.data?.length ?? 0})
      </h2>
      <ul className="space-y-1">
        {(subs.data ?? []).map((s) => (
          <li key={s.key} className="mono flex items-center gap-2 text-xs">
            <span className="text-chrome-dim">{statusGlyph(s.status)}</span>
            <Link
              href={`/tasks/${s.key}`}
              className="text-accent hover:underline"
            >
              {s.key}
            </Link>
            <span className="truncate text-chrome">{s.title}</span>
          </li>
        ))}
        {subs.data?.length === 0 && !adding && (
          <li className="mono text-[11px] text-chrome-dim">no subtasks</li>
        )}
      </ul>
      {canManage && (adding ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim()) add.mutate();
          }}
          className="flex items-center gap-1"
        >
          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="subtask title"
            className="flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          <button
            type="submit"
            disabled={!title.trim() || add.isPending}
            className="mono rounded bg-accent px-2 py-1 text-[10px] text-accent-fg disabled:opacity-50"
          >
            add
          </button>
          <button
            type="button"
            onClick={() => { setAdding(false); setTitle(""); }}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Cancel"
          >
            <X size={12} />
          </button>
        </form>
      ) : (
        <button
          type="button"
          onClick={() => setAdding(true)}
          className="mono flex items-center gap-1 text-[11px] text-chrome-dim hover:text-chrome"
        >
          <Plus size={11} /> add subtask
        </button>
      ))}
    </section>
  );
}

// ─── Links ──────────────────────────────────────────────────────────────────

export function LinksPanel({
  taskKey,
  canManage,
}: {
  taskKey: string;
  canManage: boolean;
}) {
  const qc = useQueryClient();
  const links = useQuery({
    queryKey: ["links", taskKey],
    queryFn: () => listLinks(taskKey),
  });
  const remove = useMutation({
    mutationFn: ({ to, kind }: { to: string; kind: LinkKind }) =>
      removeLink(taskKey, to, kind),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["links", taskKey] }),
  });
  const [picking, setPicking] = useState(false);

  return (
    <section className="space-y-2">
      <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        <LinkIcon size={11} /> links ({links.data?.length ?? 0})
      </h2>
      <ul className="space-y-1">
        {(links.data ?? []).map((l) => (
          <li
            key={`${l.direction}-${l.kind}-${l.other_task_key}`}
            className="mono flex items-center gap-2 text-xs"
          >
            <span className="text-chrome-dim">
              {l.direction === "outgoing" ? KIND_LABEL[l.kind] : INCOMING_LABEL[l.kind]}
            </span>
            <Link href={`/tasks/${l.other_task_key}`} className="text-accent hover:underline">
              {l.other_task_key}
            </Link>
            <span className="truncate text-chrome">{l.other_task_title}</span>
            {canManage && l.direction === "outgoing" && (
              <button
                type="button"
                onClick={() => remove.mutate({ to: l.other_task_key, kind: l.kind })}
                aria-label="remove link"
                className="ml-auto text-chrome-dim hover:text-red-300"
              >
                <Trash2 size={11} />
              </button>
            )}
          </li>
        ))}
        {links.data?.length === 0 && !picking && (
          <li className="mono text-[11px] text-chrome-dim">no links</li>
        )}
      </ul>
      {canManage && (picking ? (
        <AddLinkRow
          taskKey={taskKey}
          onDone={() => {
            setPicking(false);
            qc.invalidateQueries({ queryKey: ["links", taskKey] });
          }}
        />
      ) : (
        <button
          type="button"
          onClick={() => setPicking(true)}
          className="mono flex items-center gap-1 text-[11px] text-chrome-dim hover:text-chrome"
        >
          <Plus size={11} /> add link
        </button>
      ))}
    </section>
  );
}

function AddLinkRow({
  taskKey,
  onDone,
}: {
  taskKey: string;
  onDone: () => void;
}) {
  const [kind, setKind] = useState<LinkKind>("relates_to");
  const [q, setQ] = useState("");
  const [picked, setPicked] = useState<string | null>(null);

  const hits = useQuery({
    queryKey: ["link-search", q],
    queryFn: () => search(q, 5),
    enabled: q.length >= 2,
    staleTime: 5_000,
  });

  const add = useMutation({
    mutationFn: () => addLink(taskKey, picked!, kind),
    onSuccess: onDone,
  });

  return (
    <div className="space-y-1 rounded border border-white/10 bg-ink-subtle p-2">
      <div className="flex items-center gap-1">
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value as LinkKind)}
          className="mono rounded border border-white/10 bg-ink px-1 py-0.5 text-[11px] text-chrome"
        >
          {(Object.keys(KIND_LABEL) as LinkKind[]).map((k) => (
            <option key={k} value={k}>{KIND_LABEL[k]}</option>
          ))}
        </select>
        <input
          autoFocus
          value={q}
          onChange={(e) => { setQ(e.target.value); setPicked(null); }}
          placeholder="search tasks by key or title…"
          className="flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
        />
        <button
          type="button"
          onClick={onDone}
          className="text-chrome-dim hover:text-chrome"
          aria-label="Cancel"
        >
          <X size={12} />
        </button>
      </div>
      {picked ? (
        <div className="mono flex items-center gap-2 px-1 text-[11px]">
          <span className="text-accent">{picked}</span>
          <button
            type="button"
            onClick={() => add.mutate()}
            disabled={add.isPending}
            className="ml-auto mono rounded bg-accent px-2 py-0.5 text-[10px] text-accent-fg disabled:opacity-50"
          >
            link
          </button>
          <button
            type="button"
            onClick={() => { setPicked(null); setQ(""); }}
            className="text-chrome-dim hover:text-chrome"
          >
            change
          </button>
        </div>
      ) : (
        <ul className="max-h-40 overflow-y-auto">
          {(hits.data?.tasks ?? []).map((t) => (
            <li key={t.key}>
              <button
                type="button"
                onClick={() => setPicked(t.key)}
                className="mono flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs hover:bg-white/5"
              >
                <span className="text-chrome-dim">{t.key}</span>
                <span className="truncate text-chrome">{t.title}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function statusGlyph(s: string): string {
  switch (s) {
    case "done": return "✓";
    case "in_progress": return "▸";
    case "review": return "⌖";
    default: return "○";
  }
}
