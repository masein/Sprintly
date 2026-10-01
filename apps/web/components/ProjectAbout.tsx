"use client";

// "About this project" on the dashboard: a description the lead writes, and
// the documents that belong to the project rather than to one task —
// roadmaps, requirement briefs, contracts (QA report 6: "enable leaders to
// add and edit project descriptions as well as upload essential project
// documents directly within the dashboard"). Everyone on the project reads
// and downloads; leads edit and upload.

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, FileText, Pencil, Trash2, Upload } from "lucide-react";
import {
  deleteProjectDocument,
  editProject,
  listProjectDocuments,
  uploadProjectDocument,
  type Project,
} from "@/lib/projects";
import type { ApiError } from "@/lib/api";
import { Markdown } from "./Markdown";

export function ProjectAbout({ project, canEdit }: { project: Project; canEdit: boolean }) {
  return (
    <section
      aria-label="about this project"
      className="mb-6 grid grid-cols-1 gap-4 rounded-lg border border-white/10 bg-ink-subtle p-4 lg:grid-cols-[1fr_340px]"
    >
      <Description project={project} canEdit={canEdit} />
      <Documents projectKey={project.key} canEdit={canEdit} />
    </section>
  );
}

function Description({ project, canEdit }: { project: Project; canEdit: boolean }) {
  const qc = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [body, setBody] = useState(project.description ?? "");
  const [error, setError] = useState<string | null>(null);
  const save = useMutation({
    mutationFn: () => editProject(project.key, { description: body }),
    onSuccess: () => {
      setEditing(false);
      setError(null);
      void qc.invalidateQueries({ queryKey: ["project", project.key] });
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "couldn't save"),
  });

  return (
    <div className="min-w-0" data-project-description>
      <div className="mb-1 flex items-center justify-between">
        <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">about this project</h2>
        {canEdit && !editing && (
          <button
            type="button"
            onClick={() => {
              setBody(project.description ?? "");
              setEditing(true);
            }}
            aria-label="edit the project description"
            className="text-chrome-dim hover:text-chrome"
          >
            <Pencil size={13} />
          </button>
        )}
      </div>
      {editing ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate();
          }}
          className="space-y-2"
        >
          <textarea
            autoFocus
            value={body}
            onChange={(e) => setBody(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                setEditing(false);
              }
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                save.mutate();
              }
            }}
            maxLength={4000}
            rows={6}
            aria-label="project description"
            placeholder="What is this project, who is it for, what does done look like? Markdown works."
            className="block w-full rounded border border-white/10 bg-ink px-2 py-1.5 text-sm text-chrome focus:border-accent focus:outline-none"
          />
          {error && <div className="mono text-[11px] text-red-300">{error}</div>}
          <div className="flex items-center justify-end gap-2">
            <span className="mono mr-auto text-[10px] text-chrome-dim">⌘↵ to save · esc to cancel</span>
            <button
              type="button"
              onClick={() => setEditing(false)}
              className="mono text-xs text-chrome-dim hover:text-chrome"
            >
              cancel
            </button>
            <button
              type="submit"
              disabled={save.isPending}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              {save.isPending ? "…" : "save"}
            </button>
          </div>
        </form>
      ) : project.description ? (
        <div className="text-sm">
          <Markdown>{project.description}</Markdown>
        </div>
      ) : (
        <p className="mono text-xs text-chrome-dim">
          {canEdit
            ? "No description yet. Say what this project is for — future you will thank you."
            : "No description yet."}
        </p>
      )}
    </div>
  );
}

function Documents({ projectKey, canEdit }: { projectKey: string; canEdit: boolean }) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["project-documents", projectKey],
    queryFn: () => listProjectDocuments(projectKey),
  });
  const [uploading, setUploading] = useState<{ name: string; progress: number; error: string | null }[]>([]);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const refresh = () => qc.invalidateQueries({ queryKey: ["project-documents", projectKey] });
  const del = useMutation({ mutationFn: deleteProjectDocument, onSuccess: refresh });

  async function start(files: FileList | null) {
    if (!files) return;
    for (const f of Array.from(files)) {
      setUploading((u) => [...u, { name: f.name, progress: 0, error: null }]);
      try {
        await uploadProjectDocument(projectKey, f, (p) =>
          setUploading((u) => u.map((x) => (x.name === f.name ? { ...x, progress: p } : x))),
        );
        setUploading((u) => u.filter((x) => x.name !== f.name));
        void refresh();
      } catch (e) {
        setUploading((u) =>
          u.map((x) => (x.name === f.name ? { ...x, error: (e as Error).message } : x)),
        );
      }
    }
  }

  const docs = (q.data ?? []).filter((d) => d.status === "ready");
  return (
    <div className="min-w-0 space-y-2" aria-label="project documents" role="region">
      <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        <FileText size={11} /> documents ({docs.length})
      </h2>
      {canEdit && (
        <div
          onDragOver={(e) => {
            e.preventDefault();
            setOver(true);
          }}
          onDragLeave={() => setOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setOver(false);
            void start(e.dataTransfer.files);
          }}
          onClick={() => input.current?.click()}
          className={`mono flex cursor-pointer items-center justify-center gap-2 rounded border border-dashed p-3 text-[11px] transition ${
            over ? "border-accent bg-accent/5 text-chrome" : "border-white/10 text-chrome-dim hover:text-chrome"
          }`}
        >
          <Upload size={12} /> roadmaps, briefs, specs — drop or click
          <input
            ref={input}
            type="file"
            multiple
            className="hidden"
            aria-label="upload project documents"
            onChange={(e) => void start(e.target.files)}
          />
        </div>
      )}
      <ul className="space-y-1">
        {uploading.map((u) => (
          <li key={u.name} className="mono flex items-center gap-2 rounded border border-white/10 px-2 py-1 text-xs">
            <Upload size={11} className="text-chrome-dim" />
            <span className="truncate" title={u.name}>{u.name}</span>
            <span className={`ml-auto ${u.error ? "text-red-300" : "text-chrome-dim"}`}>
              {u.error ?? `${Math.round(u.progress * 100)}%`}
            </span>
          </li>
        ))}
        {docs.map((d) => (
          <li
            key={d.id}
            className="mono flex items-center gap-2 rounded border border-white/10 bg-ink px-2 py-1.5 text-xs"
            data-project-document={d.filename}
          >
            <FileText size={12} className="shrink-0 text-chrome-dim" />
            <span className="min-w-0 flex-1 truncate text-chrome" title={d.filename}>
              {d.filename}
            </span>
            <span className="shrink-0 text-[10px] text-chrome-dim">
              {fmtSize(d.size_bytes)}
              {d.uploader_handle ? ` · @${d.uploader_handle}` : ""}
            </span>
            <a
              href={d.download_url!}
              target="_blank"
              rel="noreferrer"
              aria-label={`Download ${d.filename}`}
              className="shrink-0 text-accent hover:opacity-80"
            >
              <Download size={12} />
            </a>
            {canEdit && (
              <button
                type="button"
                onClick={() => del.mutate(d.id)}
                aria-label={`remove ${d.filename}`}
                className="shrink-0 text-chrome-dim hover:text-red-300"
              >
                <Trash2 size={12} />
              </button>
            )}
          </li>
        ))}
        {q.data && docs.length === 0 && uploading.length === 0 && (
          <li className="mono text-[11px] text-chrome-dim">no documents yet</li>
        )}
      </ul>
    </div>
  );
}

function fmtSize(b: number | null): string {
  if (b == null) return "";
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1024 / 1024).toFixed(1)} MB`;
}
