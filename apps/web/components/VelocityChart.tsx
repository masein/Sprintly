"use client";

// Velocity bar chart over closed sprints.

import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { VelocityPoint } from "@/lib/dashboards";

export function VelocityChart({ points }: { points: VelocityPoint[] }) {
  if (points.length === 0) {
    return (
      <div className="mono rounded border border-dashed border-white/10 p-6 text-center text-xs text-chrome-dim">
        no completed sprints yet
      </div>
    );
  }
  const data = points.map((p) => ({
    name: p.name.length > 14 ? `${p.name.slice(0, 13)}…` : p.name,
    velocity: p.velocity_points,
  }));
  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
        velocity (last {points.length} sprints)
      </div>
      <div className="h-48">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 8 }}>
            <CartesianGrid stroke="#ffffff10" strokeDasharray="3 3" />
            <XAxis
              dataKey="name"
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
            <Bar dataKey="velocity" fill="#7c5cff" radius={[3, 3, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
