// The little "3 subtasks" pill. One component so the board, backlog, and sprint
// lists agree on what it looks like and what it's called — the accessible
// name is what the e2e specs key on. Renders nothing for zero: a "0" badge is
// noise on every card that isn't broken down.

import { ListTree } from "lucide-react";

export function SubtaskBadge({ count, className = "" }: { count: number; className?: string }) {
  if (!count || count < 1) return null;
  const label = `${count} subtask${count === 1 ? "" : "s"}`;
  return (
    <span
      data-subtask-badge
      aria-label={label}
      title={label}
      className={`mono inline-flex shrink-0 items-center gap-0.5 rounded border border-white/10 px-1 py-px text-[10px] leading-none text-chrome-dim ${className}`}
    >
      <ListTree size={9} aria-hidden />
      {count}
    </span>
  );
}
