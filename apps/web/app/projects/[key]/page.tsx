"use client";

import { useEffect, useState } from "react";
import { useRouter, useParams } from "next/navigation";
import Link from "next/link";
import { Archive, ArchiveRestore, Pencil, Tags } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Board } from "@/components/Board";
import { LabelsManager } from "@/components/LabelsManager";
import { projectIcon } from "@/components/CreateProjectModal";
import {
  archiveProject,
  editProject,
  getProject,
  listBoards,
  unarchiveProject,
  type Board as BoardModel,
  type Project,
} from "@/lib/projects";
import type { ApiError } from "@/lib/api";

export default function ProjectPage() {
  const router = useRouter();
  const params = useParams<{ key: string }>();
  const projectKey = params?.key ?? "";

  const [project, setProject] = useState<Project | null>(null);
  const [boards, setBoards] = useState<BoardModel[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editingName, setEditingName] = useState(false);
  const [showLabels, setShowLabels] = useState(false);

  async function reload() {
    try {
      const [p, b] = await Promise.all([
        getProject(projectKey),
        listBoards(projectKey),
      ]);
      setProject(p);
      setBoards(b);
    } catch (e) {
      const err = e as unknown as ApiError;
      if (err.status === 401) {
        router.push("/login");
        return;
      }
      if (err.status === 403 || err.status === 404) {
        setError("This project doesn't exist, or you don't have access.");
        return;
      }
      setError(err.message);
    }
  }

  useEffect(() => {
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectKey]);

  if (error) {
    return (
      <AppShell currentProjectKey={projectKey}>
        <div className="mono rounded border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-200">
          {error}
        </div>
        <Link href="/projects" className="mono mt-4 inline-block text-xs text-accent">
          ← back to projects
        </Link>
      </AppShell>
    );
  }

  if (!project || !boards) {
    return (
      <AppShell currentProjectKey={projectKey}>
        <div className="mono text-sm text-chrome-dim">git fetch --rebase your-stuff…</div>
      </AppShell>
    );
  }

  const Icon = projectIcon(project.icon);
  const canManage = project.your_role === "lead";
  const defaultBoard = boards.find((b) => b.is_default) ?? boards[0];

  return (
    <AppShell currentProjectKey={projectKey}>
      <header className="mb-6 flex items-center gap-4">
        <div
          className="flex h-12 w-12 items-center justify-center rounded-lg"
          style={{ background: `${project.color}20`, color: project.color }}
        >
          <Icon size={24} />
        </div>
        <div className="flex-1">
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            {project.key} · {project.member_count}{" "}
            {project.member_count === 1 ? "member" : "members"}
            {project.your_role && (
              <> · you are <span className="text-chrome">{project.your_role}</span></>
            )}
          </div>
          {editingName ? (
            <InlineName
              initial={project.name}
              onSave={async (name) => {
                const updated = await editProject(project.key, { name });
                setProject(updated);
                setEditingName(false);
              }}
              onCancel={() => setEditingName(false)}
            />
          ) : (
            <h1 className="flex items-center gap-2 text-3xl font-semibold">
              {project.name}
              {canManage && (
                <button
                  type="button"
                  onClick={() => setEditingName(true)}
                  className="text-chrome-dim hover:text-chrome"
                  aria-label="Rename project"
                >
                  <Pencil size={16} />
                </button>
              )}
              {project.archived_at && (
                <span className="mono ml-2 inline-flex items-center gap-1 rounded border border-white/10 px-2 py-0.5 text-xs uppercase text-chrome-dim">
                  <Archive size={11} /> archived
                </span>
              )}
            </h1>
          )}
        </div>

        <Link
          href={`/projects/${project.key}/dashboard`}
          className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          dashboard →
        </Link>
        <Link
          href={`/projects/${project.key}/sprints`}
          className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          sprints →
        </Link>
        <Link
          href={`/projects/${project.key}/vault`}
          className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          vault →
        </Link>

        {canManage && (
          <button
            type="button"
            onClick={() => setShowLabels(true)}
            className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <Tags size={14} /> labels
          </button>
        )}

        {canManage && (
          <button
            type="button"
            onClick={async () => {
              try {
                if (project.archived_at) await unarchiveProject(project.key);
                else await archiveProject(project.key);
                await reload();
              } catch (e) {
                setError((e as unknown as ApiError).message);
              }
            }}
            className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            {project.archived_at ? (
              <><ArchiveRestore size={14} /> unarchive</>
            ) : (
              <><Archive size={14} /> archive</>
            )}
          </button>
        )}
      </header>

      {showLabels && (
        <LabelsManager projectKey={project.key} onClose={() => setShowLabels(false)} />
      )}

      {defaultBoard ? (
        <Board
          projectKey={project.key}
          projectId={project.id}
          board={defaultBoard}
          canManage={canManage && !project.archived_at}
          onBoardChange={(next) => {
            setBoards(
              boards.map((b) => (b.id === next.id ? next : b)),
            );
          }}
        />
      ) : (
        <div className="mono rounded border border-dashed border-white/10 p-8 text-center text-sm text-chrome-dim">
          no boards yet — that&apos;s unusual; recreate the project
        </div>
      )}
    </AppShell>
  );
}

function InlineName({
  initial,
  onSave,
  onCancel,
}: {
  initial: string;
  onSave: (name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial);
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        if (name && name !== initial) await onSave(name);
        else onCancel();
      }}
      className="flex items-center gap-2"
    >
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        className="w-full rounded border border-white/10 bg-ink px-2 py-1 text-2xl font-semibold text-chrome focus:border-accent focus:outline-none"
      />
      <button type="submit" className="mono text-xs text-accent">save</button>
      <button type="button" onClick={onCancel} className="mono text-xs text-chrome-dim">cancel</button>
    </form>
  );
}
