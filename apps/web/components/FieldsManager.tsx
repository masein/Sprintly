"use client";

// Manage a project's custom field schema: add, delete. Opened from the
// project header. Type is immutable after creation — delete and recreate
// rather than silently invalidating stored values.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2, X } from "lucide-react";
import {
  createField,
  deleteField,
  listProjectFields,
  type FieldType,
} from "@/lib/fields";
import type { ApiError } from "@/lib/api";

const TYPES: FieldType[] = ["text", "number", "select", "date"];

export function FieldsManager({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["project-fields", projectKey],
    queryFn: () => listProjectFields(projectKey),
    retry: false,
  });
  const invalidate = () =>
    qc.invalidateQueries({ queryKey: ["project-fields", projectKey] });

  const [name, setName] = useState("");
  const [type, setType] = useState<FieldType>("text");
  const [optionsText, setOptionsText] = useState("");
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () =>
      createField(projectKey, {
        name: name.trim(),
        type,
        options:
          type === "select"
            ? optionsText.split(",").map((o) => o.trim()).filter(Boolean)
            : undefined,
      }),
    onSuccess: () => {
      setName("");
      setOptionsText("");
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteField(id),
    onSuccess: invalidate,
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });

  const defs = q.data ?? [];

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-md space-y-4 rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              {projectKey} · custom fields
            </div>
            <h2 className="text-xl font-semibold">Field schema</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        <ul className="space-y-1">
          {defs.map((f) => (
            <li
              key={f.id}
              className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5"
            >
              <span className="mono text-xs text-chrome">{f.name}</span>
              <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim">
                {f.type}
              </span>
              {f.type === "select" && (
                <span className="mono truncate text-[10px] text-chrome-dim">
                  {f.options.join(" · ")}
                </span>
              )}
              <button
                type="button"
                aria-label={`delete ${f.name}`}
                onClick={() => {
                  if (
                    confirm(
                      `Delete "${f.name}"? Every value set on a task goes with it. No undo.`,
                    )
                  )
                    remove.mutate(f.id);
                }}
                className="ml-auto text-chrome-dim hover:text-red-300"
              >
                <Trash2 size={13} />
              </button>
            </li>
          ))}
          {defs.length === 0 && (
            <li className="mono text-[11px] text-chrome-dim">
              no fields yet — schema design is the fun part
            </li>
          )}
        </ul>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) add.mutate();
          }}
          className="space-y-2 border-t border-white/10 pt-3"
        >
          <div className="flex items-center gap-2">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={40}
              placeholder="new field"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <select
              value={type}
              onChange={(e) => setType(e.target.value as FieldType)}
              aria-label="field type"
              className="mono rounded border border-white/10 bg-ink px-1.5 py-1 text-xs text-chrome"
            >
              {TYPES.map((t) => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
            <button
              type="submit"
              disabled={add.isPending || !name.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              add
            </button>
          </div>
          {type === "select" && (
            <input
              value={optionsText}
              onChange={(e) => setOptionsText(e.target.value)}
              placeholder="options, comma-separated"
              className="mono w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
          )}
          {error && (
            <div className="mono text-[11px] text-red-300">{error}</div>
          )}
        </form>
      </div>
    </div>
  );
}
