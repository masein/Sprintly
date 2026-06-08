"use client";

// Sidebar panel listing the commits / pull requests linked to a task by the
// GitHub integration. Hidden when there's nothing linked.

import { useQuery } from "@tanstack/react-query";
import { GitBranch, GitCommit, GitPullRequest } from "lucide-react";
import { listGitLinks, type GitLink } from "@/lib/integrations";

const ICON: Record<GitLink["kind"], React.ComponentType<{ size?: string | number }>> = {
  commit: GitCommit,
  pull_request: GitPullRequest,
  branch: GitBranch,
};

function stateClass(state: string | null): string {
  switch (state) {
    case "merged":
      return "border-violet-500/30 text-violet-300";
    case "closed":
      return "border-red-500/30 text-red-300";
    case "open":
      return "border-emerald-500/30 text-emerald-300";
    default:
      return "border-white/10 text-chrome-dim";
  }
}

export function GitLinksPanel({ taskKey }: { taskKey: string }) {
  const q = useQuery({
    queryKey: ["git-links", taskKey],
    queryFn: () => listGitLinks(taskKey),
    retry: false,
  });
  const links = q.data ?? [];
  if (links.length === 0) return null;

  return (
    <section className="space-y-2">
      <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        git activity
      </h2>
      <ul className="space-y-1">
        {links.map((l) => {
          const Icon = ICON[l.kind] ?? GitCommit;
          return (
            <li key={l.id} className="mono flex items-start gap-2 text-xs">
              <Icon size={12} />
              <span className="min-w-0 flex-1">
                {l.url ? (
                  <a
                    href={l.url}
                    target="_blank"
                    rel="noreferrer"
                    className="text-accent hover:underline"
                  >
                    {l.external_ref}
                  </a>
                ) : (
                  <span className="text-chrome">{l.external_ref}</span>
                )}
                {l.title && (
                  <span className="ml-1 truncate text-chrome-dim">{l.title}</span>
                )}
              </span>
              {l.kind === "pull_request" && l.state && (
                <span
                  className={`rounded border px-1 py-0.5 text-[10px] uppercase ${stateClass(l.state)}`}
                >
                  {l.state}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
