"use client";

// Cmd-K command palette. Single source of truth for navigation, search, and
// the easter eggs.
//
// Modes (detected from the leading character of the query):
//   ""       → mixed: tasks + projects + actions + nav
//   ">…"     → actions only
//   "?"      → help (shortcut sheet)
//   ":q"     → close palette
//   ":wq"    → close palette (alias)
//   "sudo …" → "Permission denied" easter egg
//   "rm -rf" → "Nice try." easter egg
//   "konami" → enable CRT mode for 10 min, then close
//
// Open with Cmd/Ctrl+K. "/" anywhere outside an input also opens in search mode.

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import {
  Search, ArrowRight, Hash, FolderKanban, User, Sparkles,
  Settings, LogOut, FolderPlus, AtSign, ListTodo,
} from "lucide-react";
import { search, type SearchHits } from "@/lib/search";
import { logout } from "@/lib/auth-bundle";

const CHORD_TIMEOUT_MS = 1500;

type Mode = "mixed" | "actions" | "help" | "ee-sudo" | "ee-rm" | "ee-konami" | "quit";

function detectMode(q: string): Mode {
  const lower = q.trim().toLowerCase();
  if (lower === ":q" || lower === ":wq") return "quit";
  if (lower === "konami") return "ee-konami";
  if (lower.startsWith("sudo")) return "ee-sudo";
  if (lower.startsWith("rm -rf")) return "ee-rm";
  if (q.startsWith(">")) return "actions";
  if (q.startsWith("?")) return "help";
  return "mixed";
}

type Action = {
  id: string;
  label: string;
  hint?: string;
  icon: React.ComponentType<{ size?: string | number }>;
  run: (ctx: ActionCtx) => void | Promise<void>;
};

type ActionCtx = {
  router: ReturnType<typeof useRouter>;
  close: () => void;
};

const ACTIONS: Action[] = [
  {
    id: "go-projects",
    label: "go to projects",
    hint: "g p",
    icon: FolderKanban,
    run: ({ router, close }) => { router.push("/projects"); close(); },
  },
  {
    id: "go-my-tasks",
    label: "go to my tasks",
    hint: "g m",
    icon: ListTodo,
    run: ({ router, close }) => { router.push("/me/tasks"); close(); },
  },
  {
    id: "go-my-day",
    label: "go to my day",
    hint: "g d",
    icon: ListTodo,
    run: ({ router, close }) => { router.push("/me/day"); close(); },
  },
  {
    id: "go-settings",
    label: "go to settings",
    hint: "g s",
    icon: Settings,
    run: ({ router, close }) => { router.push("/settings"); close(); },
  },
  {
    id: "new-project",
    label: "new project",
    icon: FolderPlus,
    run: ({ router, close }) => { router.push("/projects?new=1"); close(); },
  },
  {
    id: "open-docs",
    label: "read the docs (it's an achievement)",
    icon: Sparkles,
    run: ({ router, close }) => { router.push("/docs"); close(); },
  },
  {
    id: "open-admin",
    label: "admin panel (admin only)",
    icon: Settings,
    run: ({ router, close }) => { router.push("/admin"); close(); },
  },
  {
    id: "go-achievements",
    label: "my achievements",
    icon: Sparkles,
    run: ({ router, close }) => { router.push("/me/achievements"); close(); },
  },
  {
    id: "logout",
    label: "logout",
    icon: LogOut,
    run: async ({ router, close }) => {
      await logout().catch(() => {});
      close();
      router.push("/login");
    },
  },
];

export function CommandPalette({
  open,
  onOpenChange,
  initialQuery = "",
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initialQuery?: string;
}) {
  const router = useRouter();
  const [q, setQ] = useState(initialQuery);
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // Reset / refill when toggling open.
  useEffect(() => {
    if (!open) {
      setQ("");
      setActiveIdx(0);
    } else {
      setQ(initialQuery);
      setTimeout(() => inputRef.current?.focus(), 10);
    }
  }, [open, initialQuery]);

  const mode = detectMode(q);

  // Special-mode handling: konami fires once and closes.
  useEffect(() => {
    if (mode === "ee-konami") {
      enableCrtMode();
      const t = setTimeout(() => onOpenChange(false), 800);
      return () => clearTimeout(t);
    }
    if (mode === "quit") {
      onOpenChange(false);
    }
  }, [mode, onOpenChange]);

  // Search query (debounced via TanStack's staleTime + a manual delay).
  const searchTerm = useMemo(() => {
    if (mode !== "mixed") return "";
    const t = q.trim();
    return t.length >= 2 ? t : "";
  }, [q, mode]);

  const searchQ = useQuery({
    queryKey: ["palette-search", searchTerm],
    queryFn: () => search(searchTerm, 6),
    enabled: !!searchTerm,
    staleTime: 5_000,
  });

  // Flat result list per mode for keyboard nav.
  const flat = useMemo(() => buildResults(mode, q, searchQ.data ?? null), [mode, q, searchQ.data]);

  useEffect(() => setActiveIdx(0), [mode, q]);

  function close() { onOpenChange(false); }

  function runActive() {
    const item = flat[activeIdx];
    if (!item) return;
    item.run({ router, close });
  }

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-[60] flex items-start justify-center bg-black/60 p-4 pt-[12vh]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="w-full max-w-xl overflow-hidden rounded-xl border border-white/10 bg-ink-subtle shadow-2xl">
        <div className="flex items-center gap-2 border-b border-white/10 px-3 py-2">
          <Search size={14} className="text-chrome-dim" />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActiveIdx((i) => Math.min(flat.length - 1, i + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActiveIdx((i) => Math.max(0, i - 1));
              } else if (e.key === "Enter") {
                e.preventDefault();
                runActive();
              } else if (e.key === "Escape") {
                e.preventDefault();
                close();
              }
            }}
            placeholder="search tasks · > actions · ? help · or just type"
            className="mono w-full bg-transparent text-sm text-chrome outline-none placeholder:text-chrome-dim"
          />
          <kbd className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-chrome-dim">
            esc
          </kbd>
        </div>

        <div className="max-h-[60vh] overflow-y-auto p-1">
          {mode === "help" && <HelpPanel />}
          {mode === "ee-sudo" && (
            <EgoEgg>
              Permission denied — you are not in the sudoers file. This incident
              will be reported.
            </EgoEgg>
          )}
          {mode === "ee-rm" && <EgoEgg>Nice try.</EgoEgg>}
          {mode === "ee-konami" && (
            <EgoEgg>CRT mode enabled. Touch grass in 10 minutes.</EgoEgg>
          )}
          {(mode === "mixed" || mode === "actions") && (
            <ResultList items={flat} active={activeIdx} onActive={setActiveIdx} onPick={runActive} />
          )}
        </div>
      </div>
    </div>
  );
}

// ─── result types ───────────────────────────────────────────────────────────

type RowItem = {
  kind: "task" | "project" | "user" | "action";
  id: string;
  icon: React.ComponentType<{ size?: string | number }>;
  label: string;
  hint?: string;
  sub?: string;
  run: (ctx: ActionCtx) => void | Promise<void>;
};

function buildResults(mode: Mode, q: string, hits: SearchHits | null): RowItem[] {
  const lower = q.replace(/^>/, "").trim().toLowerCase();
  const out: RowItem[] = [];

  // Actions first when in actions mode; trimmed by query.
  if (mode === "actions" || mode === "mixed") {
    for (const a of ACTIONS) {
      if (mode === "actions" && lower && !a.label.toLowerCase().includes(lower)) continue;
      out.push({
        kind: "action",
        id: a.id,
        icon: a.icon,
        label: a.label,
        hint: a.hint,
        run: a.run,
      });
    }
  }

  if (mode === "mixed" && hits) {
    for (const t of hits.tasks) {
      out.push({
        kind: "task",
        id: `task-${t.key}`,
        icon: Hash,
        label: t.title,
        hint: t.key,
        sub: `${t.project_key} · ${t.status} · ${t.priority}`,
        run: ({ router, close }) => {
          router.push(`/tasks/${t.key}`);
          close();
        },
      });
    }
    for (const p of hits.projects) {
      out.push({
        kind: "project",
        id: `project-${p.key}`,
        icon: FolderKanban,
        label: p.name,
        hint: p.key,
        run: ({ router, close }) => {
          router.push(`/projects/${p.key}`);
          close();
        },
      });
    }
    for (const u of hits.users) {
      out.push({
        kind: "user",
        id: `user-${u.id}`,
        icon: AtSign,
        label: u.display_name,
        hint: `@${u.handle}`,
        run: ({ close }) => {
          // No user-profile page yet (M10). Closing is the polite default.
          close();
        },
      });
    }
  }

  return out;
}

// ─── rendering helpers ──────────────────────────────────────────────────────

function ResultList({
  items,
  active,
  onActive,
  onPick,
}: {
  items: RowItem[];
  active: number;
  onActive: (i: number) => void;
  onPick: () => void;
}) {
  if (items.length === 0) {
    return (
      <div className="mono p-6 text-center text-xs text-chrome-dim">
        nothing matched. try{" "}
        <span className="text-chrome">{">"}</span> for actions,{" "}
        <span className="text-chrome">?</span> for help.
      </div>
    );
  }
  return (
    <ul>
      {items.map((it, i) => {
        const Icon = it.icon;
        const isActive = i === active;
        return (
          <li
            key={it.id}
            onMouseEnter={() => onActive(i)}
            onClick={onPick}
            className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm transition ${
              isActive ? "bg-accent/10 text-chrome" : "text-chrome-dim hover:bg-white/5"
            }`}
          >
            <Icon size={14} />
            <span className={`truncate ${isActive ? "" : "text-chrome"}`}>{it.label}</span>
            {it.sub && (
              <span className="mono ml-2 truncate text-[10px] text-chrome-dim">{it.sub}</span>
            )}
            {it.hint && (
              <kbd className="mono ml-auto rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-chrome-dim">
                {it.hint}
              </kbd>
            )}
            {isActive && <ArrowRight size={12} className="ml-1 text-chrome-dim" />}
          </li>
        );
      })}
    </ul>
  );
}

function HelpPanel() {
  const shortcuts: [string, string][] = [
    ["⌘K / Ctrl+K", "open this palette"],
    ["/", "open palette in search mode"],
    ["g p", "go to projects"],
    ["g m", "go to my tasks"],
    ["g d", "go to my day"],
    ["g s", "go to settings"],
    ["c", "new card in the leftmost column (on board)"],
    ["?", "show this help"],
    [":q", "close any modal / palette"],
    [":wq", "save & close (in editing contexts)"],
    ["esc", "close palette"],
  ];
  return (
    <div className="space-y-1 p-2 text-sm">
      <div className="mono px-2 pb-1 pt-2 text-[10px] uppercase tracking-widest text-chrome-dim">
        keyboard shortcuts
      </div>
      <ul className="space-y-0.5">
        {shortcuts.map(([k, label]) => (
          <li
            key={k}
            className="flex items-center gap-2 rounded px-2 py-1 text-chrome-dim"
          >
            <kbd className="mono inline-block min-w-[3.5rem] rounded border border-white/10 px-1.5 py-0.5 text-center text-[10px] text-chrome">
              {k}
            </kbd>
            <span>{label}</span>
          </li>
        ))}
      </ul>
      <div className="mono mt-3 flex items-center gap-2 px-2 text-[10px] text-chrome-dim">
        <Sparkles size={11} /> there are easter eggs. you&apos;ll find them.
      </div>
    </div>
  );
}

function EgoEgg({ children }: { children: React.ReactNode }) {
  return (
    <div className="mono px-3 py-4 text-sm text-chrome">{children}</div>
  );
}

// ─── CRT mode (konami easter egg) ───────────────────────────────────────────

function enableCrtMode() {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.classList.add("crt-mode");
  // Auto-disable after 10 minutes.
  window.setTimeout(() => root.classList.remove("crt-mode"), 10 * 60 * 1000);
}
