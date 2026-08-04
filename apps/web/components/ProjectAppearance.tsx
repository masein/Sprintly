"use client";

// Icon + colour picker for an existing project. Both were creation-time-only
// choices: the API accepted `icon` and `color` on PATCH from day one, but the
// only place to set them was the create dialog, so a mis-picked icon was
// permanent (QA: "ability to change the project icon and its color").
//
// Leads only — same gate as renaming the project.

import { useEffect, useRef, useState } from "react";
import { Check } from "lucide-react";
import { ICON_IDS, projectIcon } from "./CreateProjectModal";
import { editProject, type Project } from "@/lib/projects";
import type { ApiError } from "@/lib/api";

const COLORS = ["#7c5cff", "#22d3ee", "#10b981", "#f59e0b", "#ef4444", "#ec4899"];

export function ProjectAppearance({
  project,
  canEdit,
  onChanged,
}: {
  project: Project;
  canEdit: boolean;
  onChanged: (p: Project) => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const Icon = projectIcon(project.icon);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  async function save(patch: { icon?: string; color?: string }) {
    setBusy(true);
    try {
      onChanged(await editProject(project.key, patch));
    } catch (e) {
      alert((e as unknown as ApiError).message);
    } finally {
      setBusy(false);
    }
  }

  const tile = (
    <div
      className="flex h-12 w-12 shrink-0 items-center justify-center rounded-lg"
      style={{ background: `${project.color}20`, color: project.color }}
    >
      <Icon size={24} />
    </div>
  );

  if (!canEdit) return tile;

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-label="project appearance"
        aria-expanded={open}
        title="change the icon and colour"
        className="rounded-lg transition hover:ring-2 hover:ring-white/20"
      >
        {tile}
      </button>

      {open && (
        <div className="absolute left-0 z-30 mt-2 w-64 space-y-3 rounded-lg border border-white/10 bg-ink p-3 shadow-xl">
          <div className="space-y-1.5">
            <span className="mono block text-[10px] uppercase tracking-widest text-chrome-dim">
              icon
            </span>
            <div className="grid grid-cols-6 gap-1">
              {ICON_IDS.map((id) => {
                const I = projectIcon(id);
                const on = id === project.icon;
                return (
                  <button
                    type="button"
                    key={id}
                    disabled={busy}
                    onClick={() => save({ icon: id })}
                    aria-label={`icon ${id}`}
                    aria-pressed={on}
                    className={`flex h-8 items-center justify-center rounded border transition disabled:opacity-50 ${
                      on ? "border-accent bg-accent/10 text-accent" : "border-white/10 text-chrome-dim hover:text-chrome"
                    }`}
                  >
                    <I size={14} />
                  </button>
                );
              })}
            </div>
          </div>

          <div className="space-y-1.5">
            <span className="mono block text-[10px] uppercase tracking-widest text-chrome-dim">
              colour
            </span>
            <div className="flex gap-2">
              {COLORS.map((c) => (
                <button
                  type="button"
                  key={c}
                  disabled={busy}
                  onClick={() => save({ color: c })}
                  aria-label={`colour ${c}`}
                  aria-pressed={c === project.color}
                  style={{ background: c }}
                  className={`flex h-6 w-6 items-center justify-center rounded-full border-2 transition disabled:opacity-50 ${
                    c === project.color ? "border-white" : "border-transparent"
                  }`}
                >
                  {c === project.color && <Check size={12} className="text-white" />}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
