"use client";

// A small donut with the percentage in the middle. SVG by hand — one circle
// for the track, one for the arc — because a charting library for a ring is
// a lot of library.

export function ProgressRing({
  value,
  size = 84,
  stroke = 9,
  color = "#10b981",
  label,
}: {
  /** 0–100. */
  value: number;
  size?: number;
  stroke?: number;
  color?: string;
  /** Accessible description, e.g. "82% of sprint tasks done". */
  label: string;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const v = Math.max(0, Math.min(100, value));
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      role="img"
      aria-label={label}
      className="shrink-0"
    >
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="#ffffff14" strokeWidth={stroke} />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={color}
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={`${(v / 100) * c} ${c}`}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
      <text
        x="50%"
        y="50%"
        dominantBaseline="central"
        textAnchor="middle"
        className="fill-chrome font-semibold"
        style={{ fontSize: size * 0.22 }}
      >
        {Math.round(v)}%
      </text>
    </svg>
  );
}
