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
import { ChevronDown, FolderPlus, FolderKanban, Menu, Search, X } from "lucide-react";
import { listProjects, type Project } from "@/lib/projects";
import { SessionBadge, SessionMenuContents } from "./SessionBadge";
import { RunningTimerChip } from "./RunningTimerChip";
import { CoffeeMeter } from "./CoffeeMeter";
import { NotificationBell } from "./NotificationBell";
import { OfflineBanner } from "./OfflineBanner";

export function AppShell({
  currentProjectKey,
  children,
}: {
  currentProjectKey?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen">
      <OfflineBanner />
      <TopBar currentProjectKey={currentProjectKey} />
      <main className="mx-auto max-w-7xl px-4 py-6 sm:px-6 sm:py-8">{children}</main>
    </div>
  );
}

function TopBar({ currentProjectKey }: { currentProjectKey?: string }) {
  const [menuOpen, setMenuOpen] = useState(false);

  // Close the mobile menu on outside click / Esc, same pattern as the switcher.
  useEffect(() => {
    if (!menuOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setMenuOpen(false);
    const onClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest("[data-session-menu]")) setMenuOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onClick);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onClick);
    };
  }, [menuOpen]);

  return (
    <header className="sticky top-0 z-20 border-b border-white/10 bg-ink/80 backdrop-blur">
      <div className="mx-auto flex h-12 max-w-7xl items-center gap-2 px-4 sm:gap-3 sm:px-6">
        <Link href="/" className="mono shrink-0 text-sm tracking-tight">
          <span className="font-semibold">sprintly</span>
          <span className="text-chrome-dim">/</span>
        </Link>

        <ProjectSwitcher currentProjectKey={currentProjectKey} />

        {/* Query search. The label folds away on narrow screens — the header
            row is exactly where things used to overflow. */}
        <Link
          href="/search"
          className="mono inline-flex shrink-0 items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          <Search size={11} />
          <span className="hidden sm:inline">search</span>
        </Link>

        <div className="ml-auto flex shrink-0 items-center gap-2 sm:gap-3">
          {/* The coffee meter is a nicety; hide it on the narrowest screens. */}
          <span className="hidden sm:inline-flex">
            <CoffeeMeter />
          </span>
          <RunningTimerChip />
          <NotificationBell />

          {/* Desktop: session actions inline. Below lg they'd overflow the row
              (that was the whole bug), so they collapse into a menu instead. */}
          <span className="hidden lg:inline-flex">
            <SessionBadge />
          </span>

          <div data-session-menu className="relative lg:hidden">
            <button
              type="button"
              onClick={() => setMenuOpen((v) => !v)}
              className="flex h-7 w-7 items-center justify-center rounded border border-white/10 text-chrome-dim hover:border-white/20 hover:text-chrome"
              aria-label={menuOpen ? "Close menu" : "Open menu"}
              aria-expanded={menuOpen}
            >
              {menuOpen ? <X size={14} /> : <Menu size={14} />}
            </button>
            {menuOpen && (
              <div
                role="menu"
                className="absolute right-0 top-full mt-1 w-56 rounded border border-white/10 bg-ink-subtle p-1 shadow-xl"
              >
                <SessionMenuContents onNavigate={() => setMenuOpen(false)} />
              </div>
            )}
          </div>
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
    <div data-project-switcher className="relative min-w-0 shrink">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="mono flex min-w-0 max-w-[45vw] items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2.5 py-1 text-xs text-chrome hover:border-white/20 sm:max-w-xs"
        aria-expanded={open}
      >
        <FolderKanban size={14} className="shrink-0" />
        {current ? (
          <>
            <span
              className="inline-block h-2 w-2 shrink-0 rounded-full"
              style={{ background: current.color }}
              aria-hidden
            />
            <span className="shrink-0 whitespace-nowrap">{current.key}</span>
            <span className="min-w-0 truncate whitespace-nowrap text-chrome-dim">
              — {current.name}
            </span>
          </>
        ) : (
          <span className="whitespace-nowrap text-chrome-dim">project · select…</span>
        )}
        <ChevronDown size={12} className="shrink-0 text-chrome-dim" />
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
