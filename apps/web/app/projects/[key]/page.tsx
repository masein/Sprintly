"use client";

import { useEffect, useState } from "react";
import { useRouter, useParams } from "next/navigation";
import Link from "next/link";
import { Archive, ArchiveRestore, ArrowDownUp, FileStack, GitBranch, ListChecks, Pencil, Share2, Tags, Users, Webhook } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Board } from "@/components/Board";
import { FieldsManager } from "@/components/FieldsManager";
import { GitIntegrationsManager } from "@/components/GitIntegrationsManager";
import { ImportExportModal } from "@/components/ImportExportModal";
import { LabelsManager } from "@/components/LabelsManager";
import { MembersManager } from "@/components/MembersManager";
import { PublicStatusModal } from "@/components/PublicStatusModal";
import { TemplatesManager } from "@/components/TemplatesManager";
import { WebhooksManager } from "@/components/WebhooksManager";
import { ProjectAppearance } from "@/components/ProjectAppearance";
import {
  archiveProject,
  editProject,
  getProject,
  listBoards,
  unarchiveProject,
  type Board as BoardModel,
  type Project,
} from "@/lib/projects";
import { me } from "@/lib/auth-bundle";
import { subscribe } from "@/lib/ws";
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
  const [showFields, setShowFields] = useState(false);
  const [showGit, setShowGit] = useState(false);
  const [showWebhooks, setShowWebhooks] = useState(false);
  const [showTemplates, setShowTemplates] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [showPublic, setShowPublic] = useState(false);
  const [showMembers, setShowMembers] = useState(false);
  // Global admins can manage any project's members (the API always allowed
  // it — the UI used to show them the read-only view).
  const [isAdmin, setIsAdmin] = useState(false);

  useEffect(() => {
    void me()
      .then((u) => setIsAdmin(u.role === "admin"))
      .catch(() => {});
  }, []);

  // This page holds project/boards in plain state (not TanStack), so the
  // global WS→query-cache routing can't refresh it. Listen directly: a
  // membership/role change re-fetches, so "you are …" and the manage
  // controls update without the reported manual refresh.
  useEffect(() => {
    return subscribe((e) => {
      if (e.event === "member_changed") void reload();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectKey]);

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
    // Plain-state page (not TanStack), so the global refetch-on-focus
    // doesn't cover it — refresh on focus ourselves.
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
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

  const canManage = project.your_role === "lead";
  const defaultBoard = boards.find((b) => b.is_default) ?? boards[0];

  return (
    <AppShell currentProjectKey={projectKey}>
      <header className="mb-6 space-y-3">
        {/* Title row: gets the full width to itself so a long name/breadcrumb
            never gets starved by the nav chips below (that starvation was the
            whole bug — a shared flex-wrap row let a dozen buttons eat the
            line before the title got a look at the remaining space). */}
        <div className="flex items-start gap-3">
          <ProjectAppearance
            project={project}
            canEdit={canManage}
            onChanged={setProject}
          />
          <div className="min-w-0 flex-1">
            <div className="mono flex flex-wrap items-center gap-x-1 text-xs uppercase tracking-widest text-chrome-dim">
              <KeyEditor project={project} canManage={canManage} router={router} />
              <span>
                · {project.member_count}{" "}
                {project.member_count === 1 ? "member" : "members"}
                {project.your_role && (
                  <> · you are <span className="text-chrome">{project.your_role}</span></>
                )}
              </span>
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
              <h1 className="flex min-w-0 items-center gap-2 text-3xl font-semibold">
                <span>{project.name}</span>
                {canManage && (
                  <button
                    type="button"
                    onClick={() => setEditingName(true)}
                    className="shrink-0 text-chrome-dim hover:text-chrome"
                    aria-label="Rename project"
                  >
                    <Pencil size={16} />
                  </button>
                )}
                {project.archived_at && (
                  <span className="mono ml-2 inline-flex shrink-0 items-center gap-1 rounded border border-white/10 px-2 py-0.5 text-xs uppercase text-chrome-dim">
                    <Archive size={11} /> archived
                  </span>
                )}
              </h1>
            )}
          </div>
        </div>

        {/* Nav + management chips: their own row, free to wrap onto as many
            lines as they need without touching the title's width. */}
        <div className="flex flex-wrap items-center gap-2">
          <Link
            href={`/projects/${project.key}/dashboard`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            dashboard →
          </Link>
          <Link
            href={`/projects/${project.key}/sprints`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            sprints →
          </Link>
          <Link
            href={`/projects/${project.key}/timeline`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            timeline →
          </Link>
          <Link
            href={`/projects/${project.key}/backlog`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            backlog →
          </Link>
          <Link
            href={`/projects/${project.key}/vault`}
            className="mono inline-flex items-center gap-1 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            vault →
          </Link>

          {/* Everyone can see who's on the project; only leads can mutate
              (enforced inside the manager + by the API). */}
          <button
            type="button"
            onClick={() => setShowMembers(true)}
            className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <Users size={14} /> members
          </button>

          {canManage && (
            <button
              type="button"
              onClick={() => setShowLabels(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <Tags size={14} /> labels
            </button>
          )}

          {canManage && (
            <button
              type="button"
              onClick={() => setShowFields(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <ListChecks size={14} /> fields
            </button>
          )}

          {canManage && (
            <button
              type="button"
              onClick={() => setShowGit(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <GitBranch size={14} /> git
            </button>
          )}

          {canManage && (
            <button
              type="button"
              onClick={() => setShowTemplates(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <FileStack size={14} /> templates
            </button>
          )}

          {canManage && (
            <button
              type="button"
              onClick={() => setShowWebhooks(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <Webhook size={14} /> webhooks
            </button>
          )}

          {canManage && (
            <button
              type="button"
              onClick={() => setShowImport(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <ArrowDownUp size={14} /> import / export
            </button>
          )}

          {canManage && (
            <button
              type="button"
              onClick={() => setShowPublic(true)}
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <Share2 size={14} /> public status
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
              className="mono flex items-center gap-2 rounded border border-white/10 px-3 py-2.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              {project.archived_at ? (
                <><ArchiveRestore size={14} /> unarchive</>
              ) : (
                <><Archive size={14} /> archive</>
              )}
            </button>
          )}
        </div>
      </header>

      {showLabels && (
        <LabelsManager projectKey={project.key} onClose={() => setShowLabels(false)} />
      )}

      {showFields && (
        <FieldsManager projectKey={project.key} onClose={() => setShowFields(false)} />
      )}

      {showGit && (
        <GitIntegrationsManager projectKey={project.key} onClose={() => setShowGit(false)} />
      )}

      {showWebhooks && (
        <WebhooksManager projectKey={project.key} onClose={() => setShowWebhooks(false)} />
      )}

      {showTemplates && (
        <TemplatesManager projectKey={project.key} onClose={() => setShowTemplates(false)} />
      )}

      {showImport && (
        <ImportExportModal
          projectKey={project.key}
          onClose={() => setShowImport(false)}
          onImported={() => reload()}
        />
      )}

      {showPublic && (
        <PublicStatusModal projectKey={project.key} onClose={() => setShowPublic(false)} />
      )}

      {showMembers && (
        <MembersManager
          projectKey={project.key}
          canManage={canManage || isAdmin}
          onClose={() => setShowMembers(false)}
        />
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

// ─── Key editor (rename cascade) ─────────────────────────────────────────────
// Changing the key rewrites every task key in the project (TST-12 → OPS-12)
// in one transaction, then navigates to the new URL. Old links stop
// resolving, so the confirm() spells that out before anything is sent.

function KeyEditor({
  project,
  canManage,
  router,
}: {
  project: Project;
  canManage: boolean;
  router: ReturnType<typeof useRouter>;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(project.key);
  const [busy, setBusy] = useState(false);

  if (!canManage) return <span>{project.key}</span>;
  if (!editing) {
    return (
      <span className="inline-flex items-center gap-1">
        {project.key}
        <button
          type="button"
          onClick={() => {
            setValue(project.key);
            setEditing(true);
          }}
          aria-label="Change project key"
          title="change the project key (rewrites every task key)"
          className="text-chrome-dim hover:text-chrome"
        >
          <Pencil size={10} />
        </button>
      </span>
    );
  }
  return (
    <form
      className="inline-flex items-center gap-1"
      onSubmit={async (e) => {
        e.preventDefault();
        const next = value.trim().toUpperCase();
        if (!next || next === project.key) {
          setEditing(false);
          return;
        }
        if (
          !confirm(
            `Rename ${project.key} to ${next}?\n\nEvery task key is rewritten ` +
              `(${project.key}-12 becomes ${next}-12). Old links — bookmarks, ` +
              `commit messages, anything written down — will stop resolving. ` +
              `This is the whole project's identity; make sure the team knows.`,
          )
        ) {
          return;
        }
        setBusy(true);
        try {
          const updated = await editProject(project.key, { key: next });
          router.push(`/projects/${updated.key}`);
        } catch (err) {
          alert((err as unknown as ApiError).message);
          setBusy(false);
        }
      }}
    >
      <input
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value.toUpperCase())}
        onKeyDown={(e) => {
          if (e.key === "Escape") setEditing(false);
        }}
        maxLength={10}
        aria-label="new project key"
        className="mono w-24 rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs uppercase text-chrome focus:border-accent focus:outline-none"
      />
      <button type="submit" disabled={busy} className="text-accent hover:underline disabled:opacity-50">
        {busy ? "…" : "rename"}
      </button>
      <button
        type="button"
        onClick={() => setEditing(false)}
        className="text-chrome-dim hover:text-chrome"
      >
        :q
      </button>
    </form>
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
      className="flex min-w-0 items-center gap-2"
    >
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        className="min-w-0 flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-2xl font-semibold text-chrome focus:border-accent focus:outline-none"
      />
      <button type="submit" className="mono shrink-0 text-xs text-accent">save</button>
      <button type="button" onClick={onCancel} className="mono shrink-0 text-xs text-chrome-dim">cancel</button>
    </form>
  );
}
