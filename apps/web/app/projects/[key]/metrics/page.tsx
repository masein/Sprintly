"use client";

// Project flow metrics: lead time, throughput, WIP.

import { useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { AppShell } from "@/components/AppShell";
import { ThroughputChart } from "@/components/ThroughputChart";
import { fmtHours, getMetrics } from "@/lib/metrics";
import type { ApiError } from "@/lib/api";

const WINDOWS = [4, 8, 12, 26];

export default function MetricsPage() {
  const params = useParams<{ key: string }>();
  const key = params.key;
  const router = useRouter();
  const [weeks, setWeeks] = useState(8);

  const q = useQuery({
    queryKey: ["metrics", key, weeks],
    queryFn: () => getMetrics(key, weeks),
    retry: (n, e) =>
      (e as unknown as ApiError)?.status !== 401 &&
      (e as unknown as ApiError)?.status !== 403 &&
      n < 1,
  });

  if (q.error) {
    const e = q.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    if (e.status === 403) {
      return (
        <AppShell currentProjectKey={key}>
          <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
            You don&apos;t have access to this project.
          </div>
        </AppShell>
      );
    }
  }

  const m = q.data;
  return (
    <AppShell currentProjectKey={key}>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            {key} · flow metrics
          </div>
          <h1 className="text-3xl font-semibold">How work flows.</h1>
        </div>
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1">
            {WINDOWS.map((w) => (
              <button
                key={w}
                type="button"
                onClick={() => setWeeks(w)}
                className={`mono rounded border px-2 py-1 text-xs ${
                  weeks === w
                    ? "border-accent text-chrome"
                    : "border-white/10 text-chrome-dim hover:border-white/20 hover:text-chrome"
                }`}
              >
                {w}w
              </button>
            ))}
          </div>
          <Link
            href={`/projects/${key}/dashboard`}
            className="mono text-xs text-accent hover:underline"
          >
            dashboard →
          </Link>
        </div>
      </header>

      {q.isLoading && (
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      )}

      {m && (
        <div className="space-y-6">
          <section className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <Tile label="completed" value={`${m.lead_time.count}`} />
            <Tile label="avg lead" value={fmtHours(m.lead_time.avg_hours)} />
            <Tile label="median lead" value={fmtHours(m.lead_time.p50_hours)} />
            <Tile label="p90 lead" value={fmtHours(m.lead_time.p90_hours)} />
          </section>

          <ThroughputChart points={m.throughput} />

          <section>
            <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
              work in progress
            </div>
            <div className="grid grid-cols-3 gap-3">
              <Tile label="to do" value={`${m.wip.todo}`} />
              <Tile label="in progress" value={`${m.wip.in_progress}`} />
              <Tile label="review" value={`${m.wip.review}`} />
            </div>
          </section>
        </div>
      )}
    </AppShell>
  );
}

function Tile({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
        {label}
      </div>
      <div className="mt-1 text-2xl text-chrome">{value}</div>
    </div>
  );
}
