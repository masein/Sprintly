"use client";

// Modal for creating a new project. Drives the create flow on /projects.
// Includes a tiny color picker (5 preset accents) and an icon picker (lucide
// names from a curated list). The form auto-derives a default key from the
// project name but lets you override.

import { useEffect, useState } from "react";
import { createProject } from "@/lib/projects";
import type { ApiError } from "@/lib/api";
import {
  Folder, FolderKanban, Boxes, Bug, Sparkles, Rocket, Beaker, GitBranch,
  Cpu, Database, Wrench, X,
} from "lucide-react";

const COLORS = ["#7c5cff", "#22d3ee", "#10b981", "#f59e0b", "#ef4444", "#ec4899"];
const ICONS: Record<string, React.ComponentType<{ size?: string | number }>> = {
  folder: Folder,
  kanban: FolderKanban,
  boxes: Boxes,
  bug: Bug,
  sparkles: Sparkles,
  rocket: Rocket,
  beaker: Beaker,
  branch: GitBranch,
  cpu: Cpu,
  database: Database,
  wrench: Wrench,
};

export function CreateProjectModal({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: (key: string) => void;
}) {
  const [name, setName] = useState("");
  const [key, setKey] = useState("");
  const [keyTouched, setKeyTouched] = useState(false);
  const [icon, setIcon] = useState<string>("kanban");
  const [color, setColor] = useState<string>(COLORS[0]!);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<{ name?: string; key?: string }>({});

  useEffect(() => {
    if (!open) {
      // Reset when closed.
      setName("");
      setKey("");
      setKeyTouched(false);
      setIcon("kanban");
      setColor(COLORS[0]!);
      setError(null);
      setFieldErrors({});
    }
  }, [open]);

  // Auto-suggest the key from the name (uppercase letters/digits, max 6).
  useEffect(() => {
    if (keyTouched) return;
    const derived = name
      .toUpperCase()
      .replace(/[^A-Z0-9]/g, "")
      .slice(0, 6);
    setKey(derived);
  }, [name, keyTouched]);

  // Esc to close.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    // Inline required-field feedback instead of a silent no-op (QA F5).
    const errs: { name?: string; key?: string } = {};
    if (!name.trim()) errs.name = "Name is required.";
    if (!key.trim()) errs.key = "Key is required.";
    else if (!/^[A-Z][A-Z0-9]{1,9}$/.test(key))
      errs.key = "Key must be 2–10 uppercase letters or digits, starting with a letter.";
    setFieldErrors(errs);
    if (errs.name || errs.key) return;

    setSubmitting(true);
    setError(null);
    try {
      const p = await createProject({ key, name, icon, color });
      onCreated(p.key);
    } catch (err) {
      setError((err as unknown as ApiError).message);
    } finally {
      setSubmitting(false);
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
      <form
        onSubmit={submit}
        className="w-full max-w-md space-y-5 rounded-lg border border-white/10 bg-ink-subtle p-6"
      >
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              new project
            </div>
            <h2 className="text-xl font-semibold">$ git init project</h2>
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

        <label className="block space-y-1.5">
          <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
            Name
          </span>
          <input
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              if (fieldErrors.name) setFieldErrors((p) => ({ ...p, name: undefined }));
            }}
            maxLength={80}
            placeholder="e.g. Sprintly Internal"
            aria-invalid={!!fieldErrors.name}
            className={`block w-full rounded border bg-ink px-3 py-2 text-sm text-chrome outline-none focus:ring-1 placeholder:text-chrome-dim/50 placeholder:italic ${
              fieldErrors.name
                ? "border-red-500/60 focus:border-red-500 focus:ring-red-500"
                : "border-white/10 focus:border-accent focus:ring-accent"
            }`}
          />
          {fieldErrors.name && (
            <span className="mono block text-xs text-red-300">{fieldErrors.name}</span>
          )}
        </label>

        <label className="block space-y-1.5">
          <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
            Key (uppercase, used in task IDs like {key || "WEB"}-142)
          </span>
          <input
            value={key}
            onChange={(e) => {
              setKey(e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "").slice(0, 10));
              setKeyTouched(true);
              if (fieldErrors.key) setFieldErrors((p) => ({ ...p, key: undefined }));
            }}
            placeholder="e.g. SPRT"
            aria-invalid={!!fieldErrors.key}
            className={`mono block w-full rounded border bg-ink px-3 py-2 text-sm text-chrome outline-none focus:ring-1 placeholder:text-chrome-dim/50 placeholder:italic ${
              fieldErrors.key
                ? "border-red-500/60 focus:border-red-500 focus:ring-red-500"
                : "border-white/10 focus:border-accent focus:ring-accent"
            }`}
          />
          {fieldErrors.key && (
            <span className="mono block text-xs text-red-300">{fieldErrors.key}</span>
          )}
        </label>

        <div className="space-y-1.5">
          <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
            Color
          </span>
          <div className="flex gap-2">
            {COLORS.map((c) => (
              <button
                type="button"
                key={c}
                onClick={() => setColor(c)}
                aria-label={`color ${c}`}
                aria-pressed={color === c}
                style={{ background: c }}
                className={`h-7 w-7 rounded-full border-2 transition ${
                  color === c ? "border-white" : "border-transparent"
                }`}
              />
            ))}
          </div>
        </div>

        <div className="space-y-1.5">
          <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
            Icon
          </span>
          <div className="grid grid-cols-6 gap-1.5">
            {Object.entries(ICONS).map(([id, Icon]) => (
              <button
                type="button"
                key={id}
                onClick={() => setIcon(id)}
                aria-pressed={icon === id}
                aria-label={`icon ${id}`}
                className={`flex h-9 items-center justify-center rounded border transition ${
                  icon === id
                    ? "border-accent bg-accent/10 text-chrome"
                    : "border-white/10 text-chrome-dim hover:border-white/20 hover:text-chrome"
                }`}
              >
                <Icon size={16} />
              </button>
            ))}
          </div>
        </div>

        {error && (
          <div className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200">
            {error}
          </div>
        )}

        <div className="flex items-center justify-end gap-3 pt-1">
          <button
            type="button"
            onClick={onClose}
            className="mono text-xs text-chrome-dim hover:text-chrome"
          >
            :q cancel
          </button>
          <button
            type="submit"
            disabled={submitting}
            className="mono rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {submitting ? "spinning up…" : "$ git init project"}
          </button>
        </div>
      </form>
    </div>
  );
}

export function projectIcon(id: string) {
  return ICONS[id] ?? Folder;
}
