"use client";

// Task card on the Kanban board. Stays small on purpose — the detail page
// (Phase B) is where the heavy info lives.

import { useRef } from "react";
import { useRouter } from "next/navigation";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { Bug, Sparkles, Wrench, Beaker, Flame } from "lucide-react";
import type { Task } from "@/lib/tasks";
import { labelColorMap, listProjectLabels } from "@/lib/labels";
import { listMembers } from "@/lib/projects";
import { Avatar } from "./Avatar";

const TYPE_ICON = {
  feature: Sparkles,
  bug: Bug,
  chore: Wrench,
  spike: Beaker,
  incident: Flame,
} as const;

const PRIORITY_COLOR: Record<Task["priority"], string> = {
  p0: "#ef4444",
  p1: "#f59e0b",
  p2: "#a3a3a3",
  p3: "#6b7280",
};

export function TaskCard({
  task,
  canManage,
}: {
  task: Task;
  canManage: boolean;
}) {
  const router = useRouter();
  const sortable = useSortable({
    id: task.id,
    data: { kind: "task", task },
    disabled: !canManage,
  });
  const Icon = TYPE_ICON[task.type] ?? Sparkles;

  // Whole-card click opens the task (QA F7) — but the card is also a drag
  // handle, so distinguish a click from a drag by how far the pointer moved
  // between press and release. The dnd PointerSensor activates at 6px, so we
  // use the same threshold: anything under it is a click, not a drag.
  const downPos = useRef<{ x: number; y: number } | null>(null);
  // dnd-kit owns onPointerDown to begin drag tracking; compose with it rather
  // than overriding, so both navigation and drag keep working.
  const { onPointerDown: dndPointerDown, ...dragListeners } = sortable.listeners ?? {};
  function open() {
    router.push(`/tasks/${task.key}`);
  }

  // Tint label chips from the project's label registry (cached per project).
  const labelsQ = useQuery({
    queryKey: ["project-labels", task.project_key],
    queryFn: () => listProjectLabels(task.project_key),
    staleTime: 60_000,
    retry: false,
  });
  const colors = labelColorMap(labelsQ.data ?? []);

  // Resolve the assignee to a handle for the avatar (cached per project).
  const membersQ = useQuery({
    queryKey: ["project-members", task.project_key],
    queryFn: () => listMembers(task.project_key),
    staleTime: 60_000,
    retry: false,
    enabled: !!task.assignee_id,
  });
  const assignee = task.assignee_id
    ? (membersQ.data ?? []).find((m) => m.user_id === task.assignee_id)
    : undefined;

  const style = {
    transform: CSS.Transform.toString(sortable.transform),
    transition: sortable.transition,
    opacity: sortable.isDragging ? 0.4 : 1,
  };

  return (
    <div
      ref={sortable.setNodeRef}
      style={style}
      {...sortable.attributes}
      {...dragListeners}
      data-task-card={task.key}
      role="button"
      tabIndex={0}
      onPointerDown={(e) => {
        downPos.current = { x: e.clientX, y: e.clientY };
        dndPointerDown?.(e);
      }}
      onClick={(e) => {
        const d = downPos.current;
        if (d && Math.hypot(e.clientX - d.x, e.clientY - d.y) > 6) return; // a drag, not a click
        open();
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          open();
        }
      }}
      className="group block cursor-pointer rounded border border-white/10 bg-ink p-2.5 text-left transition hover:border-white/20 active:cursor-grabbing"
    >
      <div className="mb-1 flex items-center gap-2">
        <span
          aria-hidden
          className="inline-block h-1.5 w-1.5 flex-shrink-0 rounded-full"
          style={{ background: PRIORITY_COLOR[task.priority] }}
          title={`priority ${task.priority}`}
        />
        <Icon size={11} className="flex-shrink-0 text-chrome-dim" />
        <Link
          href={`/tasks/${task.key}`}
          onClick={(e) => e.stopPropagation()}
          onPointerDown={(e) => e.stopPropagation()}
          className="mono truncate text-[10px] text-chrome-dim hover:text-chrome"
        >
          {task.key}
        </Link>
      </div>
      <div className="line-clamp-3 text-sm leading-snug text-chrome">
        {task.title}
      </div>
      {task.labels.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {task.labels.slice(0, 3).map((l) => {
            const c = colors[l.toLowerCase()];
            return (
              <span
                key={l}
                className="mono rounded border border-white/10 px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-chrome-dim"
                style={c ? { borderColor: `${c}66`, color: c, background: `${c}14` } : undefined}
              >
                {l}
              </span>
            );
          })}
          {task.labels.length > 3 && (
            <span className="mono text-[9px] text-chrome-dim">
              +{task.labels.length - 3}
            </span>
          )}
        </div>
      )}
      {assignee && (
        <div className="mt-2 flex items-center justify-end gap-1" title={`assigned to @${assignee.handle}`}>
          <Avatar
            size={16}
            user={{
              userId: assignee.user_id,
              displayName: assignee.display_name,
              handle: assignee.handle,
              avatarUrl: assignee.avatar_url,
              avatarStyle: assignee.avatar_style,
              avatarSeed: assignee.avatar_seed,
            }}
          />
          <span className="mono text-[9px] text-chrome-dim">@{assignee.handle}</span>
        </div>
      )}
    </div>
  );
}
