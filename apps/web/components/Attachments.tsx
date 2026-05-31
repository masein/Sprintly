"use client";

// Drag/drop or click-to-upload attachments. Uses two-phase presigned PUT
// behind `uploadAttachment` so the file never streams through the API.

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Paperclip, Download, Trash2, Upload, X } from "lucide-react";
import {
  deleteAttachment,
  listAttachments,
  uploadAttachment,
  type Attachment,
} from "@/lib/task-detail";

export function Attachments({ taskKey, canManage }: { taskKey: string; canManage: boolean }) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["attachments", taskKey],
    queryFn: () => listAttachments(taskKey),
  });

  const [uploads, setUploads] = useState<UploadState[]>([]);
  const fileInput = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);

  const del = useMutation({
    mutationFn: (id: string) => deleteAttachment(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["attachments", taskKey] }),
  });

  async function start(files: FileList | null) {
    if (!files || files.length === 0) return;
    const next: UploadState[] = Array.from(files).map((f) => ({
      file: f,
      progress: 0,
      error: null,
    }));
    setUploads((u) => [...u, ...next]);

    for (const item of next) {
      try {
        await uploadAttachment(taskKey, item.file, (p) => {
          setUploads((u) =>
            u.map((x) => (x.file === item.file ? { ...x, progress: p } : x)),
          );
        });
        setUploads((u) => u.filter((x) => x.file !== item.file));
        qc.invalidateQueries({ queryKey: ["attachments", taskKey] });
      } catch (e) {
        setUploads((u) =>
          u.map((x) =>
            x.file === item.file ? { ...x, error: (e as Error).message } : x,
          ),
        );
      }
    }
  }

  return (
    <section className="space-y-3">
      <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        <Paperclip size={11} /> attachments ({q.data?.length ?? 0})
      </h2>

      {canManage && (
        <div
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(true);
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(false);
            void start(e.dataTransfer.files);
          }}
          onClick={() => fileInput.current?.click()}
          className={`flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-dashed p-4 text-xs transition ${
            dragOver
              ? "border-accent bg-accent/5 text-chrome"
              : "border-white/10 text-chrome-dim hover:border-white/20 hover:text-chrome"
          }`}
        >
          <Upload size={14} />
          <span className="mono">drop files here, or click to choose</span>
          <input
            ref={fileInput}
            type="file"
            multiple
            className="hidden"
            onChange={(e) => start(e.target.files)}
          />
        </div>
      )}

      {uploads.length > 0 && (
        <ul className="space-y-1.5">
          {uploads.map((u) => (
            <li
              key={u.file.name + u.file.size}
              className="mono flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2 py-1.5 text-xs"
            >
              <Upload size={12} className="text-chrome-dim" />
              <span className="truncate">{u.file.name}</span>
              <span className="ml-auto text-chrome-dim">
                {u.error ? `error: ${u.error}` : `${Math.round(u.progress * 100)}%`}
              </span>
            </li>
          ))}
        </ul>
      )}

      {q.data?.length === 0 && uploads.length === 0 && (
        <div className="mono rounded border border-dashed border-white/10 p-3 text-center text-[11px] text-chrome-dim">
          no files yet
        </div>
      )}

      <ul className="space-y-1.5">
        {(q.data ?? []).map((a) => (
          <Row
            key={a.id}
            a={a}
            canDelete={canManage}
            onDelete={() => del.mutate(a.id)}
          />
        ))}
      </ul>
    </section>
  );
}

function Row({
  a,
  canDelete,
  onDelete,
}: {
  a: Attachment;
  canDelete: boolean;
  onDelete: () => void;
}) {
  return (
    <li className="mono flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2 py-1.5 text-xs">
      <Paperclip size={12} className="text-chrome-dim" />
      <span className="truncate flex-1">{a.filename}</span>
      <span className="text-chrome-dim">{fmtSize(a.size_bytes)}</span>
      {a.download_url ? (
        <a
          href={a.download_url}
          target="_blank"
          rel="noreferrer"
          className="text-accent hover:opacity-80"
          aria-label={`Download ${a.filename}`}
        >
          <Download size={12} />
        </a>
      ) : (
        <span className="text-chrome-dim">pending…</span>
      )}
      {canDelete && (
        <button
          type="button"
          onClick={onDelete}
          className="text-chrome-dim hover:text-red-300"
          aria-label="Delete attachment"
        >
          <Trash2 size={12} />
        </button>
      )}
    </li>
  );
}

function fmtSize(b: number | null): string {
  if (b == null) return "";
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1024 / 1024).toFixed(1)} MB`;
}

type UploadState = {
  file: File;
  progress: number;
  error: string | null;
};
