"use client";

// Assignee picker with a search box. A native <select> is fine for three
// members and unusable for thirty — QA hit exactly that ("can search while
// assigning the new task to a person") on a twelve-member project.
//
// Keeps the native select's contract where it matters: one accessible name
// ("assignee"), keyboard reachable, and nothing happens until you pick.

import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { Avatar } from "./Avatar";
import type { Member } from "@/lib/projects";

export function AssigneePicker({
  members,
  value,
  onChange,
  disabled,
}: {
  members: Member[];
  /** Currently assigned user id, or null for unassigned. */
  value: string | null;
  onChange: (userId: string | null) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const box = useRef<HTMLDivElement>(null);

  const current = members.find((m) => m.user_id === value) ?? null;
  const matches = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return members;
    return members.filter(
      (m) =>
        m.handle.toLowerCase().includes(needle) ||
        m.display_name.toLowerCase().includes(needle),
    );
  }, [members, q]);

  // Click-away and Escape close it; both are what people expect from a select.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  function pick(id: string | null) {
    setOpen(false);
    setQ("");
    if (id !== value) onChange(id);
  }

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        aria-label="assignee"
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className="mono flex max-w-[10rem] items-center gap-1 rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome hover:border-white/20 disabled:opacity-50"
      >
        <span className="truncate">
          {current ? `@${current.handle}` : "unassigned"}
        </span>
        <ChevronDown size={11} className="shrink-0 text-chrome-dim" aria-hidden />
      </button>

      {open && (
        <div className="absolute right-0 z-30 mt-1 w-60 rounded border border-white/10 bg-ink p-1 shadow-xl">
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              // Enter commits the top match — typing a name and hitting
              // Enter is how everyone drives a search box. Only when the
              // query actually narrowed things down; Enter on an empty
              // search has no obvious winner to pick.
              if (e.key !== "Enter") return;
              e.preventDefault();
              const first = matches[0];
              if (q.trim() && first) pick(first.user_id);
            }}
            placeholder="search people…"
            aria-label="search assignee"
            className="mono mb-1 block w-full rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          <ul role="listbox" aria-label="assignee options" className="max-h-52 overflow-y-auto">
            <li>
              <button
                type="button"
                role="option"
                aria-selected={value === null}
                onClick={() => pick(null)}
                className="mono flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs text-chrome-dim hover:bg-white/5"
              >
                {value === null && <Check size={11} className="text-accent" />}
                unassigned
              </button>
            </li>
            {matches.map((m) => (
              <li key={m.user_id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={m.user_id === value}
                  onClick={() => pick(m.user_id)}
                  className="mono flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-white/5"
                >
                  <Avatar
                    size={16}
                    user={{
                      userId: m.user_id,
                      handle: m.handle,
                      displayName: m.display_name,
                      avatarUrl: m.avatar_url,
                      avatarStyle: m.avatar_style,
                      avatarSeed: m.avatar_seed,
                    }}
                  />
                  <span className="truncate text-chrome">@{m.handle}</span>
                  {m.user_id === value && (
                    <Check size={11} className="ml-auto shrink-0 text-accent" />
                  )}
                </button>
              </li>
            ))}
            {matches.length === 0 && (
              <li className="mono px-2 py-1 text-[11px] text-chrome-dim">
                nobody matches “{q}”
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
