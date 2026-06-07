"use client";

// Shared chrome for authed pages. Top bar with:
//   - Sprintly wordmark (→ /)
//   - Project switcher dropdown (current project highlighted)
//   - Session badge on the right
//
// Cmd-K palette lands in M9. For now the switcher is a plain dropdown.

import { useEffect, useState } from "react";
import { useRouter, usePathname } from "next/navigation";
import Link from "next/link";
import { ChevronDown, FolderPlus, FolderKanban } from "lucide-react";
import { listProjects, type Project } from "@/lib/projects";
import { SessionBadge } from "./SessionBadge";
import { RunningTimerChip } from "./RunningTimerChip";
import { CoffeeMeter } from "./CoffeeMeter";
import { NotificationBell } from "./NotificationBell";

export function AppShell({
  currentProjectKey,
  children,
}: {
  currentProjectKey?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen">
      <TopBar currentProjectKey={currentProjectKey} />
      <main className="mx-auto max-w-7xl px-6 py-8">{children}</main>
    </div>
  );
}

function TopBar({ currentProjectKey }: { currentProjectKey?: string }) {
  return (
    <header className="sticky top-0 z-20 border-b border-white/10 bg-ink/80 backdrop-blur">
      <div className="mx-auto flex h-12 max-w-7xl items-center gap-3 px-6">
        <Link href="/" className="mono text-sm tracking-tight">
          <span className="font-semibold">sprintly</span>
          <span className="text-chrome-dim">/</span>
        </Link>

        <ProjectSwitcher currentProjectKey={currentProjectKey} />

        <div className="ml-auto flex items-center gap-3">
          <CoffeeMeter />
          <RunningTimerChip />
          <NotificationBell />
          <SessionBadge />
        </div>
      </div>
    </header>
  );
}

function ProjectSwitcher({ currentProjectKey }: { currentProjectKey?: string }) {
  const router = useRouter();
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [projects, setProjects] = useState<Project[] | null>(null);

  useEffect(() => {
    if (!open) return;
    listProjects()
      .then(setProjects)
      .catch(() => setProjects([]));
  }, [open]);

  // Close on outside click / Esc.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    const onClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest("[data-project-switcher]")) setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onClick);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onClick);
    };
  }, [open]);

  const current = projects?.find((p) => p.key === currentProjectKey);

  return (
    <div data-project-switcher className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="mono flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2.5 py-1 text-xs text-chrome hover:border-white/20"
        aria-expanded={open}
      >
        <FolderKanban size={14} />
        {current ? (
          <>
            <span
              className="inline-block h-2 w-2 rounded-full"
              style={{ background: current.color }}
              aria-hidden
            />
            <span>{current.key}</span>
            <span className="text-chrome-dim">— {current.name}</span>
          </>
        ) : (
          <span className="text-chrome-dim">project · select…</span>
        )}
        <ChevronDown size={12} className="text-chrome-dim" />
      </button>

      {open && (
        <div
          role="menu"
          className="absolute left-0 top-full mt-1 w-80 rounded border border-white/10 bg-ink-subtle p-1 shadow-xl"
        >
          <div className="mono px-2 pb-1 pt-1 text-[10px] uppercase tracking-widest text-chrome-dim">
            switch project
          </div>
          {projects === null && (
            <div className="mono px-2 py-2 text-xs text-chrome-dim">
              git fetch --rebase your-stuff…
            </div>
          )}
          {projects?.length === 0 && (
            <div className="mono px-2 py-2 text-xs text-chrome-dim">
              no projects yet
            </div>
          )}
          {projects?.map((p) => (
            <button
              type="button"
              key={p.id}
              onClick={() => {
                setOpen(false);
                router.push(`/projects/${p.key}`);
              }}
              className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-white/5 ${
                p.key === currentProjectKey ? "bg-white/5" : ""
              }`}
            >
              <span
                className="inline-block h-2 w-2 flex-shrink-0 rounded-full"
                style={{ background: p.color }}
                aria-hidden
              />
              <span className="mono w-12 text-chrome-dim">{p.key}</span>
              <span className="flex-1 truncate text-chrome">{p.name}</span>
              {p.archived_at && (
                <span className="mono text-[10px] uppercase text-chrome-dim">
                  archived
                </span>
              )}
            </button>
          ))}
          <div className="my-1 border-t border-white/10" />
          <Link
            href="/projects"
            onClick={() => setOpen(false)}
            className="mono flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs text-chrome-dim hover:bg-white/5 hover:text-chrome"
          >
            <FolderKanban size={12} /> all projects
          </Link>
          <Link
            href="/projects?new=1"
            onClick={() => setOpen(false)}
            className="mono flex w-full items-center gap-2 rounded px-2 py-1.5 text-xs text-accent hover:bg-white/5"
          >
            <FolderPlus size={12} /> new project
          </Link>
          {pathname && (
            <div className="mono mt-1 px-2 pt-1 text-[10px] text-chrome-dim">
              esc to close
            </div>
          )}
        </div>
      )}
    </div>
  );
}
