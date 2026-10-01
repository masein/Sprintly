"use client";

// Deleting a project: say exactly what goes, make the person type the key,
// then do it (QA report 6: "a safe project deletion workflow with appropriate
// confirmation safeguards"). The server checks the typed key as well, and the
// delete is soft — an admin can restore it from admin → projects.

import { useState } from "react";
import { Trash2, X } from "lucide-react";
import { deleteProject, type Project } from "@/lib/projects";
import type { ApiError } from "@/lib/api";

export function DeleteProjectModal({
  project,
  onClose,
  onDeleted,
}: {
  project: Project;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const matches = typed.trim() === project.key;

  async function go() {
    if (!matches || busy) return;
    setBusy(true);
    setError(null);
    try {
      await deleteProject(project.key, typed.trim());
      onDeleted();
    } catch (e) {
      setError((e as unknown as ApiError).message ?? "couldn't delete it");
      setBusy(false);
    }
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`delete ${project.key}`}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void go();
        }}
        className="w-full max-w-md space-y-4 rounded-lg border border-red-500/30 bg-ink-subtle p-6"
      >
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-red-300">
              {project.key} · danger zone
            </div>
            <h2 className="text-xl font-semibold">Delete {project.name}?</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        <ul className="mono space-y-1 text-xs text-chrome-dim">
          <li>
            · the board, every task and subtask, sprints, epics and files go with it —
            for all {project.member_count} {project.member_count === 1 ? "member" : "members"}
          </li>
          <li>· the key <span className="text-chrome">{project.key}</span> is freed for a new project</li>
          <li>· time already logged stays on people&apos;s timesheets</li>
          <li>· an admin can restore it from admin → projects</li>
        </ul>
        {!project.archived_at && (
          <p className="mono rounded border border-white/10 p-2 text-[11px] text-chrome-dim">
            Only want it out of the way? <span className="text-chrome">Archive</span> keeps it
            read-only and visible.
          </p>
        )}

        <label className="block space-y-1">
          <span className="mono text-xs text-chrome-dim">
            type <span className="text-chrome">{project.key}</span> to confirm
          </span>
          <input
            autoFocus
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            aria-label="type the project key to confirm"
            autoComplete="off"
            spellCheck={false}
            className="mono w-full rounded border border-white/10 bg-ink px-2 py-1.5 text-sm text-chrome focus:border-red-400 focus:outline-none"
          />
        </label>

        {error && <div className="mono text-[11px] text-red-300">{error}</div>}

        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="mono rounded px-3 py-1.5 text-xs text-chrome-dim hover:text-chrome"
          >
            :q keep it
          </button>
          <button
            type="submit"
            disabled={!matches || busy}
            className="mono flex items-center gap-1.5 rounded bg-red-600 px-3 py-1.5 text-xs font-medium text-white disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Trash2 size={12} /> {busy ? "deleting…" : `delete ${project.key}`}
          </button>
        </div>
      </form>
    </div>
  );
}
