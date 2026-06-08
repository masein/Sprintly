"use client";

// Weekly throughput (tasks completed per week).

import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { ThroughputPoint } from "@/lib/metrics";

export function ThroughputChart({ points }: { points: ThroughputPoint[] }) {
  if (points.length === 0) {
    return (
      <div className="mono rounded border border-dashed border-white/10 p-6 text-center text-xs text-chrome-dim">
        nothing shipped in this window
      </div>
    );
  }
  const data = points.map((p) => ({
    name: p.week_start.slice(5), // MM-DD
    count: p.count,
  }));
  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
        throughput (tasks done / week)
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
            <Bar dataKey="count" fill="#22d3ee" radius={[3, 3, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
