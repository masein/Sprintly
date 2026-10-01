"use client";

// One click, a shareable link to a task on the clipboard. Sits next to task
// keys wherever people reference work (QA report 6: "add a quick-copy task
// link near task identifiers").

import { useState } from "react";
import { Check, Link2 } from "lucide-react";
import { copyText } from "@/lib/clipboard";
import { taskUrl } from "@/lib/links";
import { showToast } from "@/lib/toast";

export function CopyTaskLink({
  taskKey,
  size = 12,
  className = "",
}: {
  taskKey: string;
  size?: number;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      data-copy-task-link={taskKey}
      aria-label={`copy link to ${taskKey}`}
      title={copied ? "Copied" : "Copy link"}
      // Board cards are drag handles and click targets; a copy must be neither.
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
      onClick={async (e) => {
        e.preventDefault();
        e.stopPropagation();
        const url = taskUrl(window.location.origin, taskKey);
        if (await copyText(url)) {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
          showToast(`Copied a link to ${taskKey}.`);
        } else {
          // Be honest, and still useful: the link is right there to select.
          showToast(`The clipboard said no. The link is ${url}`, { ttlMs: 10_000 });
        }
      }}
      className={`inline-flex shrink-0 items-center rounded p-0.5 text-chrome-dim transition hover:bg-white/5 hover:text-chrome ${className}`}
    >
      {copied ? <Check size={size} className="text-emerald-400" /> : <Link2 size={size} />}
    </button>
  );
}
