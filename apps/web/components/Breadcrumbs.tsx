"use client";

// The trail line every page carries above its title ("sprintly · SPD ·
// sprints"). It used to be decorative text, which meant getting from a
// sprint or a backlog back to the board took the project switcher or the
// browser's back button — inconsistent, and the thing QA kept tripping over.
// Every segment except the last is now a link; the last one is where you are.

import Link from "next/link";
import type { Route } from "next";

export type Crumb = {
  label: string;
  /** Omit for the current page — it renders as plain text. */
  href?: string;
  /** Optional leading icon, e.g. the vault glyph. */
  icon?: React.ReactNode;
};

export function Breadcrumbs({ items }: { items: Crumb[] }) {
  return (
    <nav aria-label="breadcrumb" className="mono flex flex-wrap items-center gap-1 text-xs uppercase tracking-widest text-chrome-dim">
      {items.map((c, i) => {
        const last = i === items.length - 1;
        return (
          <span key={`${c.label}-${i}`} className="flex items-center gap-1">
            {c.icon}
            {c.href && !last ? (
              <Link
                href={c.href as Route}
                className="rounded hover:text-chrome hover:underline focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent"
              >
                {c.label}
              </Link>
            ) : (
              <span className={last ? "text-chrome-dim" : undefined} aria-current={last ? "page" : undefined}>
                {c.label}
              </span>
            )}
            {!last && <span aria-hidden>·</span>}
          </span>
        );
      })}
    </nav>
  );
}

/** `sprintly · KEY · <page>` — the shape every project-scoped page wants. */
export function projectCrumbs(projectKey: string, page: string): Crumb[] {
  return [
    { label: "sprintly", href: "/" },
    { label: projectKey, href: `/projects/${projectKey}` },
    { label: page },
  ];
}
