"use client";

// Chip row above the board. Builds the existing filter DSL the API already
// understands: tokens joined by '+', e.g.
//   assignee:me+status:in_progress+priority:p0+label:backend
//
// Each chip is a (key, value) pair. The "+ filter" picker offers known keys;
// each key has a curated value list except "label" which is a free text input.

import { useState } from "react";
import { Plus, X } from "lucide-react";

export type Chip = {
  key: "assignee" | "status" | "priority" | "type" | "label";
  value: string;
};

const KEYS: Chip["key"][] = ["assignee", "status", "priority", "type", "label"];
const VALUES: Record<Exclude<Chip["key"], "label">, string[]> = {
  assignee: ["me"],
  status: ["todo", "in_progress", "review", "done"],
  priority: ["p0", "p1", "p2", "p3"],
  type: ["feature", "bug", "chore", "spike", "incident"],
};

export function toFilterDSL(chips: Chip[]): string {
  return chips.map((c) => `${c.key}:${c.value}`).join("+");
}

export function BoardFilters({
  chips,
  onChange,
}: {
  chips: Chip[];
  onChange: (next: Chip[]) => void;
}) {
  const [picking, setPicking] = useState<null | { key: Chip["key"] } | "key">(null);
  const [labelText, setLabelText] = useState("");

  function add(c: Chip) {
    // Dedupe identical chips.
    if (chips.some((x) => x.key === c.key && x.value === c.value)) {
      setPicking(null);
      return;
    }
    onChange([...chips, c]);
    setPicking(null);
    setLabelText("");
  }
  function remove(i: number) {
    onChange(chips.filter((_, idx) => idx !== i));
  }

  return (
    <div className="mb-3 flex flex-wrap items-center gap-1.5">
      {chips.map((c, i) => (
        <span
          key={`${c.key}:${c.value}`}
          className="mono inline-flex items-center gap-1 rounded border border-white/10 bg-ink-subtle px-2 py-0.5 text-[11px] text-chrome"
        >
          <span className="text-chrome-dim">{c.key}:</span>
          {c.value}
          <button
            type="button"
            onClick={() => remove(i)}
            aria-label="remove filter"
            className="text-chrome-dim hover:text-chrome"
          >
            <X size={11} />
          </button>
        </span>
      ))}

      {picking === null && (
        <button
          type="button"
          onClick={() => setPicking("key")}
          className="mono inline-flex items-center gap-1 rounded border border-dashed border-white/10 px-2 py-0.5 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          <Plus size={11} /> filter
        </button>
      )}

      {picking === "key" && (
        <div className="mono flex items-center gap-1 rounded border border-white/10 bg-ink-subtle px-1 py-0.5">
          {KEYS.map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => setPicking({ key: k })}
              className="rounded px-1.5 py-0.5 text-[11px] text-chrome-dim hover:bg-white/5 hover:text-chrome"
            >
              {k}
            </button>
          ))}
          <button
            type="button"
            onClick={() => setPicking(null)}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Cancel"
          >
            <X size={11} />
          </button>
        </div>
      )}

      {picking && picking !== "key" && (
        <div className="mono flex items-center gap-1 rounded border border-white/10 bg-ink-subtle px-1 py-0.5">
          <span className="px-1 text-[11px] text-chrome-dim">{picking.key}:</span>
          {picking.key === "label" ? (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                if (labelText.trim()) add({ key: "label", value: labelText.trim() });
              }}
              className="flex items-center gap-1"
            >
              <input
                autoFocus
                value={labelText}
                onChange={(e) => setLabelText(e.target.value)}
                placeholder="label"
                className="rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome focus:border-accent focus:outline-none"
              />
              <button
                type="submit"
                disabled={!labelText.trim()}
                className="rounded bg-accent px-1.5 py-0.5 text-[10px] text-accent-fg disabled:opacity-50"
              >
                add
              </button>
            </form>
          ) : (
            VALUES[picking.key].map((v) => (
              <button
                key={v}
                type="button"
                onClick={() => add({ key: picking.key, value: v })}
                className="rounded px-1.5 py-0.5 text-[11px] text-chrome-dim hover:bg-white/5 hover:text-chrome"
              >
                {v}
              </button>
            ))
          )}
          <button
            type="button"
            onClick={() => setPicking(null)}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Cancel"
          >
            <X size={11} />
          </button>
        </div>
      )}
    </div>
  );
}
