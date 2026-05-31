"use client";

// Compact stat tile used across dashboards.

export function StatTile({
  label,
  value,
  hint,
  accent,
}: {
  label: string;
  value: React.ReactNode;
  hint?: string;
  accent?: "default" | "warn" | "good";
}) {
  const accentColor =
    accent === "warn"
      ? "text-red-200"
      : accent === "good"
        ? "text-emerald-300"
        : "text-chrome";
  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
        {label}
      </div>
      <div className={`mt-1 text-2xl ${accentColor}`}>{value}</div>
      {hint && <div className="mono mt-0.5 text-[10px] text-chrome-dim">{hint}</div>}
    </div>
  );
}
