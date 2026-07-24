"use client";

// The "that didn't load" box. Pages render this instead of an eternal loading
// placeholder when a query fails for a reason we don't handle specially
// (401 → login redirect, 403/404 → their own pages). Honest about what the
// server said, with a retry that actually refetches. Voice per
// docs/PERSONALITY.md: no drama, no exclamation marks.

import { RotateCw } from "lucide-react";

export function LoadError({
  what = "This",
  message,
  onRetry,
}: {
  /** What failed, e.g. "The dashboard" — reads as "The dashboard didn't load." */
  what?: string;
  message?: string | null;
  onRetry?: () => void;
}) {
  return (
    <div
      role="alert"
      data-testid="load-error"
      className="mono rounded border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-200"
    >
      <div>
        {what} didn&apos;t load.{" "}
        <span className="text-red-200/75">
          {message?.trim() ? `The server said: ${message}` : "The server didn't say why."}
        </span>
      </div>
      {onRetry && (
        <button
          type="button"
          onClick={onRetry}
          className="mono mt-3 inline-flex items-center gap-1.5 rounded border border-red-500/40 px-2.5 py-1 text-xs text-red-200 hover:bg-red-500/10"
        >
          <RotateCw size={11} /> $ retry
        </button>
      )}
    </div>
  );
}
