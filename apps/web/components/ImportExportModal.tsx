"use client";

// Import a Trello/CSV board into a project, or export it (F16). Opened from the
// project header. Import always previews (dry-run) first so you see exactly what
// would change before committing.

import { useState } from "react";
import { Download, Upload, X } from "lucide-react";
import {
  exportUrl,
  importProject,
  type ImportFormat,
  type ImportReport,
} from "@/lib/importExport";
import type { ApiError } from "@/lib/api";

export function ImportExportModal({
  projectKey,
  onClose,
  onImported,
}: {
  projectKey: string;
  onClose: () => void;
  onImported: () => void;
}) {
  const [content, setContent] = useState("");
  const [fileName, setFileName] = useState<string | null>(null);
  const [format, setFormat] = useState<ImportFormat>("auto");
  const [report, setReport] = useState<ImportReport | null>(null);
  // Which action is in flight — drives a per-button loading label so a slow
  // large-file run never looks like a no-op. `null` means idle.
  const [pending, setPending] = useState<null | "preview" | "apply">(null);
  const busy = pending !== null;
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  // Jira: provision a user for each unmatched assignee/reporter (off by default).
  const [createUsers, setCreateUsers] = useState(false);
  const [tempPassword, setTempPassword] = useState("123456");

  async function readFile(file: File) {
    setError(null);
    setReport(null);
    setDone(null);
    setFileName(file.name);
    setContent(await file.text());
    if (file.name.endsWith(".json")) setFormat("trello");
    else if (file.name.endsWith(".csv")) setFormat("csv");
    else setFormat("auto");
  }

  async function run(dryRun: boolean) {
    // Guard: ignore re-entrant clicks (a second click while busy), but never
    // bail just because the file is large — only a genuinely empty box stops us.
    if (busy || !content.trim()) return;
    setPending(dryRun ? "preview" : "apply");
    setError(null);
    setDone(null);
    try {
      const res = await importProject(projectKey, {
        format,
        content,
        dry_run: dryRun,
        create_missing_users: createUsers,
        temp_password: createUsers ? tempPassword : undefined,
      });
      if (dryRun) {
        setReport(res);
      } else {
        setDone(`Imported ${res.tasks_created} task${res.tasks_created === 1 ? "" : "s"}.`);
        setReport(res);
        onImported();
      }
    } catch (e) {
      setError((e as ApiError).message ?? "import failed — check the logs and try again.");
    } finally {
      // Always clear, on success and failure, so neither button can wedge.
      setPending(null);
    }
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-lg space-y-4 rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              {projectKey} · import / export
            </div>
            <h2 className="text-xl font-semibold">Move data in and out</h2>
          </div>
          <button type="button" onClick={onClose} className="text-chrome-dim hover:text-chrome" aria-label="Close">
            <X size={18} />
          </button>
        </div>

        {/* Export */}
        <section className="space-y-2 rounded border border-white/10 p-3">
          <h3 className="mono flex items-center gap-2 text-[11px] uppercase tracking-widest text-chrome-dim">
            <Download size={12} /> export
          </h3>
          <p className="text-xs text-chrome-dim">
            A full <span className="mono">JSON</span> bundle (tasks, comments, attachment
            manifest) or a flat <span className="mono">CSV</span> of tasks.
          </p>
          <div className="flex gap-2">
            <a href={exportUrl(projectKey, "json")} className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome">
              download JSON
            </a>
            <a href={exportUrl(projectKey, "csv")} className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome">
              download CSV
            </a>
          </div>
        </section>

        {/* Import */}
        <section className="space-y-2 rounded border border-white/10 p-3">
          <h3 className="mono flex items-center gap-2 text-[11px] uppercase tracking-widest text-chrome-dim">
            <Upload size={12} /> import
          </h3>
          <p className="text-xs text-chrome-dim">
            A Trello board <span className="mono">.json</span> export, a{" "}
            <span className="mono">.csv</span> with a <span className="mono">name</span>{" "}
            column, or a <span className="mono">Jira</span> &ldquo;Export Excel CSV
            (all fields)&rdquo; export. Jira files are auto-detected and map richly —
            assignee, priority, type, sub-tasks, epics, sprints, story points, and
            comments. Re-importing the same Jira export updates cards instead of
            duplicating them (matched by issue key).
          </p>
          <label className="mono flex cursor-pointer items-center gap-2 text-xs text-chrome-dim">
            <input
              type="file"
              accept=".json,.csv,text/csv,application/json"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void readFile(f);
              }}
              className="block w-full text-xs file:mr-3 file:rounded file:border-0 file:bg-white/10 file:px-3 file:py-1 file:text-chrome hover:file:bg-white/20"
            />
          </label>
          {fileName && (
            <div className="mono text-[11px] text-chrome-dim">
              {fileName} · {content.length.toLocaleString()} bytes ·{" "}
              <select
                value={format}
                onChange={(e) => setFormat(e.target.value as ImportFormat)}
                aria-label="import format"
                className="rounded border border-white/10 bg-ink px-1 py-0.5 text-[11px] text-chrome"
              >
                <option value="auto">auto-detect</option>
                <option value="trello">trello</option>
                <option value="csv">csv</option>
                <option value="jira">jira</option>
              </select>
            </div>
          )}

          {/* Jira: optional user provisioning. */}
          <label className="mono flex items-start gap-2 text-[11px] text-chrome-dim">
            <input
              type="checkbox"
              checked={createUsers}
              onChange={(e) => {
                setCreateUsers(e.target.checked);
                setReport(null);
              }}
              className="mt-0.5"
            />
            <span>
              create missing users (Jira) — provision a Sprintly account for each
              unmatched assignee/reporter, add them to the project, and assign
              their tasks. They sign in with the temp password below and must set
              a new one.
            </span>
          </label>
          {createUsers && (
            <label className="mono flex items-center gap-2 pl-6 text-[11px] text-chrome-dim">
              temp password
              <input
                type="text"
                value={tempPassword}
                onChange={(e) => setTempPassword(e.target.value)}
                aria-label="temporary password"
                className="rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
              />
            </label>
          )}

          {report && (
            <div className="space-y-1 rounded border border-white/10 bg-ink p-2 text-[11px] text-chrome-dim">
              <div className="mono uppercase tracking-widest text-chrome-dim">
                {report.dry_run ? "preview" : "imported"}
              </div>
              <div className="mono text-chrome">
                {report.tasks_created} task{report.tasks_created === 1 ? "" : "s"}
                {report.dry_run ? " would be created" : " created"}
                {report.tasks_updated > 0 && `, ${report.tasks_updated} updated`}
              </div>
              {report.columns_created.length > 0 && (
                <div>new columns: <span className="text-chrome">{report.columns_created.join(", ")}</span></div>
              )}
              {report.columns_reused.length > 0 && (
                <div>existing columns: {report.columns_reused.join(", ")}</div>
              )}
              {report.labels_created.length > 0 && (
                <div>new labels: <span className="text-chrome">{report.labels_created.join(", ")}</span></div>
              )}
              {report.epics_created.length > 0 && (
                <div>new epics: <span className="text-chrome">{report.epics_created.join(", ")}</span></div>
              )}
              {report.sprints_created.length > 0 && (
                <div>new sprints: <span className="text-chrome">{report.sprints_created.join(", ")}</span></div>
              )}
              {report.fields_created.length > 0 && (
                <div>new fields: <span className="text-chrome">{report.fields_created.join(", ")}</span></div>
              )}
              {report.comments_created > 0 && (
                <div>{report.comments_created} comment{report.comments_created === 1 ? "" : "s"} imported</div>
              )}
              {(report.users_created > 0 || report.users_matched > 0) && (
                <div>
                  users: <span className="text-chrome">{report.users_created} created</span>,{" "}
                  {report.users_matched} matched
                </div>
              )}
              {report.warnings.map((w, i) => (
                <div key={i} className="text-amber-300">{w}</div>
              ))}
            </div>
          )}

          {error && (
            <div
              role="alert"
              className="mono rounded border border-red-500/30 bg-red-500/10 px-2 py-1.5 text-[11px] text-red-200"
            >
              {error}
            </div>
          )}
          {done && <div className="mono text-[11px] text-emerald-300">{done}</div>}

          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={busy || !content.trim()}
              onClick={() => run(true)}
              className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:text-chrome disabled:opacity-50"
            >
              {pending === "preview" ? "compiling vibes…" : "preview (dry-run)"}
            </button>
            <button
              type="button"
              disabled={busy || !report || !report.dry_run}
              onClick={() => run(false)}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
              title={report?.dry_run ? "" : "run a preview first"}
            >
              {pending === "apply" ? "nudging electrons…" : "apply import"}
            </button>
            {/* Make the gate explicit instead of a silently-dead button. */}
            {!busy && content.trim() && (!report || !report.dry_run) && (
              <span className="mono text-[10px] text-chrome-dim">preview first to apply</span>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
