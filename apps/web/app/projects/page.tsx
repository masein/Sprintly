"use client";

import { Suspense, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import { Plus, Archive } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { CreateProjectModal, projectIcon } from "@/components/CreateProjectModal";
import { listProjects, type Project } from "@/lib/projects";
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

  async function reload() {
    try {
      setProjects(await listProjects());
    } catch (e) {
      const err = e as unknown as ApiError;
      if (err.status === 401) {
        router.push("/login");
        return;
      }
      setError(err.message);
    }
  }

  useEffect(() => {
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <AppShell>
      <div className="mb-8 flex items-center justify-between">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · projects
          </div>
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
        <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {projects.map((p) => (
            <ProjectCard key={p.id} project={p} />
          ))}
        </ul>
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
  return (
    <li>
      <Link
        href={`/projects/${project.key}`}
        className="block rounded-lg border border-white/10 bg-ink-subtle p-4 transition hover:border-white/20"
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
