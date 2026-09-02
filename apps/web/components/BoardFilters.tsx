"use client";

// Chip row above the board. Builds the existing filter DSL the API already
// understands: tokens joined by '+', e.g.
//   assignee:me+status:in_progress+priority:p0+label:backend+field:severity=high
//
// Each chip is a (key, value) pair. The "+ filter" picker offers known keys;
// each key has a curated value list except "label" and "field" which take
// free text ("field" expects name=value).

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Plus, X } from "lucide-react";
import { listMembers } from "@/lib/projects";

export type Chip = {
  key: "assignee" | "status" | "priority" | "type" | "label" | "field";
  value: string;
};

const KEYS: Chip["key"][] = ["assignee", "status", "priority", "type", "label", "field"];
type FreeTextKey = "label" | "field";
const FREE_TEXT: Record<FreeTextKey, { placeholder: string; valid: (v: string) => boolean }> = {
  label: { placeholder: "label", valid: (v) => v.length > 0 },
  field: { placeholder: "name=value", valid: (v) => /^[^=]+=.+$/.test(v) },
};
const VALUES: Record<Exclude<Chip["key"], FreeTextKey>, string[]> = {
  assignee: ["me"],
  status: ["todo", "in_progress", "review", "done"],
  priority: ["p0", "p1", "p2", "p3"],
  type: ["feature", "bug", "chore", "spike", "incident"],
};

export function toFilterDSL(chips: Chip[]): string {
  return chips.map((c) => `${c.key}:${c.value}`).join("+");
}

/** Inverse of `toFilterDSL`, for restoring chips from the URL. Unknown keys
 *  and malformed tokens are dropped, not trusted — the string is editable. */
export function fromFilterDSL(dsl: string | null | undefined): Chip[] {
  if (!dsl) return [];
  const out: Chip[] = [];
  for (const token of dsl.split("+")) {
    const i = token.indexOf(":");
    if (i <= 0) continue;
    const key = token.slice(0, i) as Chip["key"];
    const value = token.slice(i + 1);
    if (!KEYS.includes(key) || !value) continue;
    if (key === "field" && !FREE_TEXT.field.valid(value)) continue;
    if (out.some((c) => c.key === key && c.value === value)) continue;
    out.push({ key, value });
  }
  return out;
}

export function BoardFilters({
  projectKey,
  chips,
  onChange,
  onClear,
}: {
  projectKey: string;
  chips: Chip[];
  onChange: (next: Chip[]) => void;
  /** Present when there is anything to clear (chips, a pinned scope, search). */
  onClear?: () => void;
}) {
  const [picking, setPicking] = useState<null | { key: Chip["key"] } | "key">(null);
  const [labelText, setLabelText] = useState("");

  // "assignee:me" used to be the only choice, which made the filter useless
  // for looking at anyone else's plate. Offer every member; the API has
  // accepted `assignee:<user id>` all along.
  const membersQ = useQuery({
    queryKey: ["project-members", projectKey],
    queryFn: () => listMembers(projectKey),
    staleTime: 60_000,
    retry: false,
  });
  const members = membersQ.data ?? [];
  const shown = (c: Chip): string => {
    if (c.key !== "assignee" || c.value === "me") return c.value;
    const m = members.find((x) => x.user_id === c.value);
    return m ? `@${m.handle}` : c.value;
  };

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
          {shown(c)}
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

      {onClear && (
        <button
          type="button"
          onClick={onClear}
          className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-0.5 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          <X size={11} /> clear filters
        </button>
      )}

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
          {picking.key === "label" || picking.key === "field" ? (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                const v = labelText.trim();
                if (FREE_TEXT[picking.key as FreeTextKey].valid(v)) {
                  add({ key: picking.key, value: v });
                }
              }}
              className="flex items-center gap-1"
            >
              <input
                autoFocus
                value={labelText}
                onChange={(e) => setLabelText(e.target.value)}
                placeholder={FREE_TEXT[picking.key as FreeTextKey].placeholder}
                className="rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome focus:border-accent focus:outline-none"
              />
              <button
                type="submit"
                disabled={!FREE_TEXT[picking.key as FreeTextKey].valid(labelText.trim())}
                className="rounded bg-accent px-1.5 py-0.5 text-[10px] text-accent-fg disabled:opacity-50"
              >
                add
              </button>
            </form>
          ) : (
            (picking.key === "assignee"
              ? [
                  { value: "me", label: "me" },
                  ...members.map((m) => ({ value: m.user_id, label: `@${m.handle}` })),
                ]
              : VALUES[picking.key].map((v) => ({ value: v, label: v }))
            ).map((v) => (
              <button
                key={v.value}
                type="button"
                onClick={() => add({ key: picking.key, value: v.value })}
                className="rounded px-1.5 py-0.5 text-[11px] text-chrome-dim hover:bg-white/5 hover:text-chrome"
              >{v.label}</button>
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
