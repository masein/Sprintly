"use client";

// Task card on the Kanban board. Stays small on purpose — the detail page
// (Phase B) is where the heavy info lives.

import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import Link from "next/link";
import { Bug, Sparkles, Wrench, Beaker, Flame } from "lucide-react";
import type { Task } from "@/lib/tasks";

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
  const sortable = useSortable({
    id: task.id,
    data: { kind: "task", task },
    disabled: !canManage,
  });
  const Icon = TYPE_ICON[task.type] ?? Sparkles;

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
      {...sortable.listeners}
      className="group block cursor-grab rounded border border-white/10 bg-ink p-2.5 text-left transition hover:border-white/20 active:cursor-grabbing"
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
          {task.labels.slice(0, 3).map((l) => (
            <span
              key={l}
              className="mono rounded border border-white/10 px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-chrome-dim"
            >
              {l}
            </span>
          ))}
          {task.labels.length > 3 && (
            <span className="mono text-[9px] text-chrome-dim">
              +{task.labels.length - 3}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
