"use client";

// A textarea that autocompletes @mentions from the project's members.
// Typing `@` (optionally followed by part of a handle) opens a small
// listbox; ↑/↓ move, Enter/Tab pick, Esc closes. Everything else behaves
// like a plain textarea — callers pass value/onChange strings as usual.
//
// The suggestion list is project members only (that's who you'd tag);
// the server resolves and notifies whoever the final text actually names.

import { useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { listMembers } from "@/lib/projects";
import { Avatar } from "./Avatar";

const HANDLE_CHARS = /[A-Za-z0-9_]/;

/** The partial mention being typed at `caret`, or null when not in one. */
function activeMention(
  text: string,
  caret: number,
): { start: number; query: string } | null {
  // Walk back from the caret over handle chars to a possible `@`.
  let i = caret;
  while (i > 0 && HANDLE_CHARS.test(text[i - 1] ?? "")) i--;
  if (i === 0 || text[i - 1] !== "@") return null;
  // An `@` glued to a word char (an email, mid-word) is not a mention —
  // same rule the server's parser applies.
  if (i >= 2 && HANDLE_CHARS.test(text[i - 2] ?? "")) return null;
  const query = text.slice(i, caret);
  if (query.length > 32) return null;
  return { start: i - 1, query };
}

export function MentionTextarea({
  value,
  onChange,
  projectKey,
  ...rest
}: Omit<
  React.TextareaHTMLAttributes<HTMLTextAreaElement>,
  "value" | "onChange"
> & {
  value: string;
  onChange: (next: string) => void;
  projectKey: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [mention, setMention] = useState<{
    start: number;
    query: string;
  } | null>(null);
  const [selected, setSelected] = useState(0);

  const members = useQuery({
    queryKey: ["members", projectKey],
    queryFn: () => listMembers(projectKey),
    enabled: !!projectKey,
    staleTime: 60_000,
  });

  const suggestions = useMemo(() => {
    if (!mention) return [];
    const q = mention.query.toLowerCase();
    return (members.data ?? [])
      .filter((m) => m.handle.toLowerCase().startsWith(q))
      .slice(0, 6);
  }, [mention, members.data]);

  const open = mention !== null && suggestions.length > 0;

  function syncMention(el: HTMLTextAreaElement) {
    const next = activeMention(el.value, el.selectionStart ?? el.value.length);
    setMention(next);
    if (!next) setSelected(0);
  }

  function pick(handle: string) {
    const el = ref.current;
    if (!el || !mention) return;
    const caret = el.selectionStart ?? value.length;
    const next = `${value.slice(0, mention.start)}@${handle} ${value.slice(caret)}`;
    onChange(next);
    setMention(null);
    setSelected(0);
    // Put the caret right after the inserted "@handle ".
    const pos = mention.start + handle.length + 2;
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(pos, pos);
    });
  }

  return (
    <div className="relative">
      <textarea
        {...rest}
        ref={ref}
        value={value}
        role="combobox"
        aria-expanded={open}
        aria-autocomplete="list"
        aria-controls={open ? "mention-listbox" : undefined}
        onChange={(e) => {
          onChange(e.target.value);
          syncMention(e.target);
        }}
        onClick={(e) => syncMention(e.currentTarget)}
        onKeyUp={(e) => {
          // Arrow-key caret moves change which mention (if any) is active.
          if (e.key.startsWith("Arrow") && !open) syncMention(e.currentTarget);
        }}
        onBlur={() => {
          // Delay so a click on a suggestion still lands.
          setTimeout(() => setMention(null), 150);
        }}
        onKeyDown={(e) => {
          if (!open) return;
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setSelected((s) => (s + 1) % suggestions.length);
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setSelected(
              (s) => (s - 1 + suggestions.length) % suggestions.length,
            );
          } else if (e.key === "Enter" || e.key === "Tab") {
            e.preventDefault();
            const s = suggestions[selected];
            if (s) pick(s.handle);
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            setMention(null);
          }
        }}
      />
      {open && (
        <ul
          id="mention-listbox"
          role="listbox"
          aria-label="mention a member"
          className="absolute left-0 top-full z-20 mt-1 w-64 overflow-hidden rounded border border-white/10 bg-ink shadow-xl"
        >
          {suggestions.map((m, i) => (
            <li key={m.user_id} role="option" aria-selected={i === selected}>
              <button
                type="button"
                // onMouseDown beats the textarea's onBlur timeout race.
                onMouseDown={(e) => {
                  e.preventDefault();
                  pick(m.handle);
                }}
                onMouseEnter={() => setSelected(i)}
                className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left ${
                  i === selected ? "bg-accent/15" : ""
                }`}
              >
                <Avatar
                  size={16}
                  user={{
                    userId: m.user_id,
                    handle: m.handle,
                    avatarUrl: m.avatar_url,
                    avatarStyle: m.avatar_style,
                    avatarSeed: m.avatar_seed,
                  }}
                />
                <span className="mono text-xs text-chrome">@{m.handle}</span>
                <span className="truncate text-xs text-chrome-dim">
                  {m.display_name}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
