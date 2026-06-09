"use client";

// Manage a project's label palette: add, recolour, delete. Opened from the board.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2, X } from "lucide-react";
import {
  createLabel,
  deleteLabel,
  listProjectLabels,
  updateLabel,
} from "@/lib/labels";
import type { ApiError } from "@/lib/api";

const SWATCHES = [
  "#7c5cff",
  "#22d3ee",
  "#10b981",
  "#f59e0b",
  "#ef4444",
  "#ec4899",
  "#a3a3a3",
];

export function LabelsManager({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["project-labels", projectKey],
    queryFn: () => listProjectLabels(projectKey),
    retry: false,
  });
  const invalidate = () =>
    qc.invalidateQueries({ queryKey: ["project-labels", projectKey] });

  const [name, setName] = useState("");
  const [color, setColor] = useState(SWATCHES[0]!);
  const [error, setError] = useState<string | null>(null);

  const add = useMutation({
    mutationFn: () => createLabel(projectKey, name.trim(), color),
    onSuccess: () => {
      setName("");
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const recolor = useMutation({
    mutationFn: (v: { id: string; color: string }) => updateLabel(v.id, { color: v.color }),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteLabel(id),
    onSuccess: invalidate,
  });

  const labels = q.data ?? [];

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
              {projectKey} · labels
            </div>
            <h2 className="text-xl font-semibold">Label palette</h2>
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
          {labels.map((l) => (
            <li
              key={l.id}
              className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5"
            >
              <span
                className="mono rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-wider"
                style={{ borderColor: `${l.color}66`, color: l.color, background: `${l.color}14` }}
              >
                {l.name}
              </span>
              <div className="ml-auto flex items-center gap-1">
                {SWATCHES.map((s) => (
                  <button
                    key={s}
                    type="button"
                    aria-label={`recolour ${l.name} ${s}`}
                    onClick={() => recolor.mutate({ id: l.id, color: s })}
                    style={{ background: s }}
                    className={`h-4 w-4 rounded-full border ${
                      l.color.toLowerCase() === s ? "border-white" : "border-transparent"
                    }`}
                  />
                ))}
                <button
                  type="button"
                  aria-label={`delete ${l.name}`}
                  onClick={() => remove.mutate(l.id)}
                  className="ml-1 text-chrome-dim hover:text-red-300"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            </li>
          ))}
          {labels.length === 0 && (
            <li className="mono text-[11px] text-chrome-dim">no labels yet</li>
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
              placeholder="new label"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <button
              type="submit"
              disabled={add.isPending || !name.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              add
            </button>
          </div>
          <div className="flex items-center gap-1.5">
            {SWATCHES.map((s) => (
              <button
                key={s}
                type="button"
                aria-label={`color ${s}`}
                onClick={() => setColor(s)}
                style={{ background: s }}
                className={`h-5 w-5 rounded-full border-2 ${
                  color === s ? "border-white" : "border-transparent"
                }`}
              />
            ))}
          </div>
          {error && (
            <div className="mono text-[11px] text-red-300">{error}</div>
          )}
        </form>
      </div>
    </div>
  );
}
