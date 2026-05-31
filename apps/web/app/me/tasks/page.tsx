"use client";

// /me/tasks — personal queue. Tasks assigned to the current user, grouped by
// status. Linked from the cmd-K palette ("g m") and from the session badge.

import { useQuery } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { Bug, Sparkles, Wrench, Beaker, Flame } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { myTasks, type MyTask } from "@/lib/search";
import type { ApiError } from "@/lib/api";

const STATUS_LABEL: Record<MyTask["status"], string> = {
  in_progress: "in progress",
  review: "in review",
  todo: "to do",
  done: "done",
};
const STATUS_ORDER: MyTask["status"][] = ["in_progress", "review", "todo", "done"];

const TYPE_ICON: Record<string, React.ComponentType<{ size?: string | number }>> = {
  feature: Sparkles,
  bug: Bug,
  chore: Wrench,
  spike: Beaker,
  incident: Flame,
};

const PRIORITY_COLOR: Record<MyTask["priority"], string> = {
  p0: "#ef4444",
  p1: "#f59e0b",
  p2: "#a3a3a3",
  p3: "#6b7280",
};

export default function MyTasksPage() {
  const router = useRouter();
  const q = useQuery({
    queryKey: ["my-tasks"],
    queryFn: () => myTasks(),
    retry: (failureCount, err) => {
      const e = err as unknown as ApiError;
      return e?.status !== 401 && failureCount < 1;
    },
  });

  if (q.error) {
    const e = q.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
  }

  const grouped = new Map<MyTask["status"], MyTask[]>(
    STATUS_ORDER.map((s) => [s, []]),
  );
  for (const t of q.data ?? []) {
    grouped.get(t.status)?.push(t);
  }

  return (
    <AppShell>
      <header className="mb-8">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · my queue
        </div>
        <h1 className="text-3xl font-semibold">What you&apos;re on the hook for.</h1>
      </header>

      {q.isLoading && (
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      )}

      {q.data && q.data.length === 0 && (
        <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
          <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
            inbox zero
          </div>
          <p className="text-chrome-dim">
            Nothing assigned. Touch grass.
          </p>
        </div>
      )}

      <div className="space-y-8">
        {STATUS_ORDER.map((s) => {
          const items = grouped.get(s) ?? [];
          if (items.length === 0) return null;
          return (
            <section key={s}>
              <h2 className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
                {STATUS_LABEL[s]} ({items.length})
              </h2>
              <ul className="space-y-1">
                {items.map((t) => (
                  <li key={t.key}>
                    <Row task={t} />
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
      </div>
    </AppShell>
  );
}

function Row({ task }: { task: MyTask }) {
  const Icon = TYPE_ICON[task.type] ?? Sparkles;
  return (
    <Link
      href={`/tasks/${task.key}`}
      className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2 transition hover:border-white/20"
    >
      <span
        aria-hidden
        className="inline-block h-1.5 w-1.5 flex-shrink-0 rounded-full"
        style={{ background: PRIORITY_COLOR[task.priority] }}
        title={`priority ${task.priority}`}
      />
      <Icon size={12} className="flex-shrink-0 text-chrome-dim" />
      <span className="mono w-20 flex-shrink-0 text-xs text-chrome-dim">
        {task.key}
      </span>
      <span className="flex-1 truncate text-sm text-chrome">{task.title}</span>
      {task.due_date && (
        <span className="mono text-[10px] text-chrome-dim">
          due {task.due_date}
        </span>
      )}
    </Link>
  );
}
