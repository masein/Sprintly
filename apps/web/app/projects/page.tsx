"use client";

import { Suspense, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import { Plus, Archive } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Breadcrumbs } from "@/components/Breadcrumbs";
import { CreateProjectModal, projectIcon } from "@/components/CreateProjectModal";
import {
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  rectSortingStrategy,
  useSortable,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";
import { listProjects, type Project } from "@/lib/projects";
import { me, patchMe } from "@/lib/auth-bundle";
import type { ApiError } from "@/lib/api";
import { Sprint } from "@/components/Sprint";

export default function ProjectsPage() {
  return (
    <Suspense fallback={null}>
      <ProjectsInner />
    </Suspense>
  );
}

function ProjectsInner() {
  const router = useRouter();
  const search = useSearchParams();
  const [projects, setProjects] = useState<Project[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(search?.get("new") === "1");

  // Per-person card order lives in the user's own settings blob, so everyone
  // arranges their own wall without touching anyone else's (QA: "can edit
  // project cards order for each person"). Unknown/new keys keep the server's
  // order at the end, so a project someone adds you to still shows up.
  const [order, setOrder] = useState<string[]>([]);

  function applyOrder(list: Project[], keys: string[]): Project[] {
    if (keys.length === 0) return list;
    const rank = new Map(keys.map((k, i) => [k, i]));
    return [...list].sort(
      (a, b) => (rank.get(a.key) ?? Number.MAX_SAFE_INTEGER) - (rank.get(b.key) ?? Number.MAX_SAFE_INTEGER),
    );
  }

  async function reload() {
    try {
      const [list, who] = await Promise.all([listProjects(), me().catch(() => null)]);
      const saved = Array.isArray(
        (who?.settings as { project_order?: unknown } | undefined)?.project_order,
      )
        ? ((who!.settings as { project_order: string[] }).project_order)
        : [];
      setOrder(saved);
      setProjects(applyOrder(list, saved));
      return;
    } catch (e) {
      const err = e as unknown as ApiError;
      if (err.status === 401) {
        router.push("/login");
        return;
      }
      setError(err.message);
    }
  }

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );

  async function onDragEnd(e: DragEndEvent) {
    const { active, over } = e;
    if (!over || active.id === over.id || !projects) return;
    const from = projects.findIndex((p) => p.key === active.id);
    const to = projects.findIndex((p) => p.key === over.id);
    if (from < 0 || to < 0) return;
    const next = arrayMove(projects, from, to);
    setProjects(next);
    const keys = next.map((p) => p.key);
    setOrder(keys);
    // Best-effort persist: the visual move already happened, and a failed
    // save just means the order isn't remembered next time — say so rather
    // than snapping the cards back under the cursor.
    try {
      await patchMe({ settings: { project_order: keys } });
    } catch {
      setError("Couldn't save that order — it'll reset on reload.");
    }
  }

  useEffect(() => {
    reload();
    // Plain-state page (not TanStack), so the global refetch-on-focus
    // doesn't cover it — refresh on focus ourselves.
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <AppShell>
      <div className="mb-8 flex items-center justify-between">
        <div>
          <Breadcrumbs items={[{ label: "sprintly", href: "/" }, { label: "projects" }]} />
          <h1 className="text-3xl font-semibold">Your projects.</h1>
        </div>
        <button
          type="button"
          onClick={() => setCreating(true)}
          className="mono flex items-center gap-2 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90"
        >
          <Plus size={14} /> new project
        </button>
      </div>

      {error && (
        <div className="mono mb-6 rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200">
          {error}
        </div>
      )}

      {projects === null ? (
        <div className="mono text-sm text-chrome-dim">
          compiling vibes…
        </div>
      ) : projects.length === 0 ? (
        <EmptyState onCreate={() => setCreating(true)} />
      ) : (
        <DndContext sensors={sensors} onDragEnd={onDragEnd}>
          <SortableContext items={projects.map((p) => p.key)} strategy={rectSortingStrategy}>
            <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 wide:grid-cols-4">
              {projects.map((p) => (
                <ProjectCard key={p.key} project={p} />
              ))}
            </ul>
          </SortableContext>
        </DndContext>
      )}

      <CreateProjectModal
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(key) => {
          setCreating(false);
          router.push(`/projects/${key}`);
        }}
      />
    </AppShell>
  );
}

function ProjectCard({ project }: { project: Project }) {
  const Icon = projectIcon(project.icon);
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } =
    useSortable({ id: project.key });
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={isDragging ? "z-20 opacity-70" : undefined}
    >
      <div className="relative">
        {/* Drag by the grip only — the card itself stays a plain link, so a
            click never turns into an accidental rearrangement. */}
        <button
          type="button"
          {...attributes}
          {...(listeners as React.DOMAttributes<HTMLButtonElement>)}
          aria-label={`reorder ${project.key}`}
          title="drag to reorder — your own arrangement"
          className="absolute right-2 top-2 z-10 cursor-grab touch-none rounded p-1 text-chrome-dim opacity-0 transition hover:text-chrome focus-visible:opacity-100 group-hover:opacity-100 active:cursor-grabbing"
        >
          <GripVertical size={13} />
        </button>
      <Link
        href={`/projects/${project.key}`}
        className="group block rounded-lg border border-white/10 bg-ink-subtle p-4 transition hover:border-white/20"
      >
        <div className="flex items-start gap-3">
          <div
            className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded"
            style={{ background: `${project.color}20`, color: project.color }}
          >
            <Icon size={20} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="mono text-xs text-chrome-dim">{project.key}</span>
              {project.archived_at && (
                <span className="mono inline-flex items-center gap-1 rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase text-chrome-dim">
                  <Archive size={10} /> archived
                </span>
              )}
              {project.your_role && (
                <span className="mono ml-auto rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-widest text-chrome-dim">
                  {project.your_role}
                </span>
              )}
            </div>
            <div className="truncate font-medium text-chrome">{project.name}</div>
            <div className="mono mt-1 text-xs text-chrome-dim">
              {project.member_count} {project.member_count === 1 ? "member" : "members"}
            </div>
          </div>
        </div>
      </Link>
      </div>
    </li>
  );
}

function EmptyState({ onCreate }: { onCreate: () => void }) {
  return (
    <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
      <Sprint mood="surprised" size={96} className="mx-auto mb-3" />
      <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
        empty repo
      </div>
      <p className="mb-4 text-chrome-dim">
        No projects yet. Spin one up — every project gets a Kanban board with{" "}
        <span className="mono">To do · In progress · Done</span> out of the box.
      </p>
      <button
        type="button"
        onClick={onCreate}
        className="mono inline-flex items-center gap-2 rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg hover:opacity-90"
      >
        <Plus size={14} /> $ git init project
      </button>
    </div>
  );
}
