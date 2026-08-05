"use client";

// One search box for every long list in the app (board, sprint tasks,
// backlog, sprints). The lists are already fully loaded client-side, so this
// filters in place — instant, no round-trip, no server-side query language
// needed to answer "where is that card".

import { Search, X } from "lucide-react";

export function ListSearch({
  value,
  onChange,
  placeholder,
  label,
  className = "",
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  /** Accessible name — also what tests target. */
  label: string;
  className?: string;
}) {
  return (
    <div
      className={`flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2 py-1.5 ${className}`}
    >
      <Search size={12} className="shrink-0 text-chrome-dim" aria-hidden />
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onChange("");
        }}
        placeholder={placeholder}
        aria-label={label}
        className="mono w-full min-w-0 bg-transparent text-xs text-chrome placeholder:text-chrome-dim/60 focus:outline-none"
      />
      {value && (
        <button
          type="button"
          onClick={() => onChange("")}
          aria-label={`clear ${label}`}
          className="shrink-0 text-chrome-dim hover:text-chrome"
        >
          <X size={12} />
        </button>
      )}
    </div>
  );
}

/** Case-insensitive match across a task's key, title, and labels. */
export function matchesTask(
  needle: string,
  t: { key: string; title: string; labels?: string[] },
): boolean {
  const q = needle.trim().toLowerCase();
  if (!q) return true;
  return (
    t.key.toLowerCase().includes(q) ||
    t.title.toLowerCase().includes(q) ||
    (t.labels ?? []).some((l) => l.toLowerCase().includes(q))
  );
}
