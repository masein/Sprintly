"use client";

// Burndown line chart. Recharts. Two lines: actual remaining (stepped) and
// ideal (straight). Compact, dark-theme friendly.

import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { BurndownPoint } from "@/lib/sprints";

export function BurndownChart({ points }: { points: BurndownPoint[] }) {
  if (points.length === 0) {
    return (
      <div className="mono rounded border border-dashed border-white/10 p-6 text-center text-xs text-chrome-dim">
        no points yet — assign tasks with story points to populate
      </div>
    );
  }
  const data = points.map((p) => ({
    date: p.date.slice(5),
    remaining: p.remaining_points,
    ideal: Number(p.ideal_points.toFixed(1)),
  }));

  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
        burndown
      </div>
      <div className="h-64">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 8 }}>
            <CartesianGrid stroke="#ffffff10" strokeDasharray="3 3" />
            <XAxis
              dataKey="date"
              stroke="#9b9ba3"
              tick={{ fontSize: 11, fontFamily: "JetBrains Mono, monospace" }}
            />
            <YAxis
              stroke="#9b9ba3"
              tick={{ fontSize: 11, fontFamily: "JetBrains Mono, monospace" }}
              allowDecimals={false}
            />
            <Tooltip
              contentStyle={{
                background: "#111114",
                border: "1px solid #ffffff20",
                borderRadius: 6,
                fontSize: 12,
                fontFamily: "JetBrains Mono, monospace",
              }}
              labelStyle={{ color: "#e6e6ea" }}
            />
            <Legend wrapperStyle={{ fontSize: 11, fontFamily: "JetBrains Mono, monospace" }} />
            <Line
              type="stepAfter"
              dataKey="remaining"
              stroke="#7c5cff"
              strokeWidth={2}
              dot={false}
              isAnimationActive={false}
              name="remaining"
            />
            <Line
              type="linear"
              dataKey="ideal"
              stroke="#9b9ba3"
              strokeDasharray="4 4"
              strokeWidth={1}
              dot={false}
              isAnimationActive={false}
              name="ideal"
            />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
