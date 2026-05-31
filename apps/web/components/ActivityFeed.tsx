"use client";

// Activity feed for a task. Reads /tasks/:key/activity and renders human
// strings per `kind`. Updates passively when the WS layer invalidates
// the query cache.

import { useQuery } from "@tanstack/react-query";
import {
  History, Move, Sparkles, User, Pencil, MessageSquare, Timer,
  Paperclip, Link as LinkIcon, Tags, Flag, Type as TypeIcon, Check, Eye,
} from "lucide-react";
import { listActivity, type Activity } from "@/lib/task-detail";

const KIND_ICON: Record<string, React.ComponentType<{ size?: string | number }>> = {
  created: Sparkles,
  moved: Move,
  assigned: User,
  unassigned: User,
  estimated: Timer,
  titled: Pencil,
  described: Pencil,
  commented: MessageSquare,
  time_logged: Timer,
  attached: Paperclip,
  linked: LinkIcon,
  labeled: Tags,
  prioritized: Flag,
  typed: TypeIcon,
  completed: Check,
  reopened: History,
  watcher_added: Eye,
  watcher_removed: Eye,
};

export function ActivityFeed({ taskKey }: { taskKey: string }) {
  const q = useQuery({
    queryKey: ["task-activity", taskKey],
    queryFn: () => listActivity(taskKey),
  });

  return (
    <section className="space-y-3">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
        activity
      </h2>
      {q.isLoading && (
        <div className="mono text-xs text-chrome-dim">compiling vibes…</div>
      )}
      {q.data?.length === 0 && (
        <div className="mono text-xs text-chrome-dim">no events yet</div>
      )}
      <ol className="space-y-2">
        {(q.data ?? []).map((a) => (
          <Row key={a.id} a={a} />
        ))}
      </ol>
    </section>
  );
}

function Row({ a }: { a: Activity }) {
  const Icon = KIND_ICON[a.kind] ?? History;
  return (
    <li className="flex items-start gap-2 text-xs">
      <span className="mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full bg-ink-muted text-chrome-dim">
        <Icon size={11} />
      </span>
      <div className="min-w-0 flex-1">
        <span className="mono text-chrome">@{a.actor_handle ?? "?"}</span>{" "}
        <span className="text-chrome-dim">{renderKind(a)}</span>
        <span className="mono ml-2 text-[10px] text-chrome-dim">
          {fmtTime(a.created_at)}
        </span>
      </div>
    </li>
  );
}

function renderKind(a: Activity): string {
  switch (a.kind) {
    case "created": return "created this task";
    case "moved": return "moved this task";
    case "assigned": return "set the assignee";
    case "unassigned": return "removed the assignee";
    case "estimated": return "set an estimate";
    case "titled": return "edited the task";
    case "described": return "edited the description";
    case "commented": return "left a comment";
    case "time_logged": return "logged time";
    case "attached": return "attached a file";
    case "linked": return "linked another task";
    case "labeled": return "changed labels";
    case "prioritized": return "changed priority";
    case "typed": return "changed type";
    case "completed": return "marked this done";
    case "reopened": return "reopened this";
    case "watcher_added": return "started watching";
    case "watcher_removed": return "stopped watching";
    default: return a.kind;
  }
}

function fmtTime(iso: string): string {
  const d = new Date(iso);
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return `${Math.floor(diff)}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return d.toISOString().slice(0, 10);
}
