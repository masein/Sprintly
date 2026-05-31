"use client";

// /timesheets — approval queue for leads and admins. Lists all submitted
// timesheets you're entitled to approve. One-click approve + CSV download.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { Check, Download } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import {
  approveTimesheet,
  csvExportUrl,
  fmtMinutes,
  fmtMoneyCents,
  pendingApprovals,
} from "@/lib/timetracking";
import type { ApiError } from "@/lib/api";

export default function ApprovalsPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["pending-approvals"],
    queryFn: () => pendingApprovals(),
    retry: (n, e) => (e as unknown as ApiError)?.status !== 401 && n < 1,
  });

  const approve = useMutation({
    mutationFn: ({ userId, periodStart }: { userId: string; periodStart: string }) =>
      approveTimesheet(userId, periodStart),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["pending-approvals"] }),
    onError: (e) => alert((e as unknown as ApiError).message),
  });

  if (q.error) {
    const e = q.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    if (e.status === 403) {
      return (
        <AppShell>
          <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
            You don&apos;t have access to this page.
          </div>
        </AppShell>
      );
    }
  }

  const items = q.data ?? [];

  return (
    <AppShell>
      <header className="mb-6">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · approvals
        </div>
        <h1 className="text-3xl font-semibold">Timesheets to review.</h1>
      </header>

      {q.isLoading && (
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      )}

      {items.length === 0 && q.isSuccess && (
        <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
          <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
            queue zero
          </div>
          <p className="text-chrome-dim">
            Nothing pending. Touch grass.
          </p>
        </div>
      )}

      <ul className="space-y-2">
        {items.map((row) => (
          <li
            key={`${row.user_id}:${row.period_start}`}
            className="flex items-center gap-4 rounded-lg border border-white/10 bg-ink-subtle px-4 py-3"
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="mono text-sm text-chrome">@{row.handle}</span>
                <span className="mono text-xs text-chrome-dim">
                  · {row.display_name}
                </span>
              </div>
              <div className="mono text-xs text-chrome-dim">
                week of {row.period_start} · {fmtMinutes(row.total_minutes)}
                {row.billable_minutes < row.total_minutes && (
                  <> · {fmtMinutes(row.billable_minutes)} billable</>
                )}
                {" · "}
                {fmtMoneyCents(row.total_pay_cents, row.currency)}
              </div>
            </div>
            <a
              href={csvExportUrl(row.user_id, row.period_start)}
              target="_blank"
              rel="noreferrer"
              className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
            >
              <Download size={11} /> csv
            </a>
            <button
              type="button"
              onClick={() =>
                approve.mutate({
                  userId: row.user_id,
                  periodStart: row.period_start,
                })
              }
              disabled={approve.isPending}
              className="mono inline-flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-xs text-accent-fg hover:opacity-90 disabled:opacity-50"
            >
              <Check size={12} /> approve
            </button>
          </li>
        ))}
      </ul>
    </AppShell>
  );
}
