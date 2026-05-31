"use client";

// Project budget burn-rate widget. Lives on the project dashboard.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Pencil, X } from "lucide-react";
import { getBurn, setProjectBudget } from "@/lib/payroll";
import { fmtMoneyCents } from "@/lib/timetracking";

const STATUS_COLOR: Record<string, string> = {
  none: "#9b9ba3",
  ok: "#10b981",
  warn: "#f59e0b",
  over: "#ef4444",
};
const STATUS_LABEL: Record<string, string> = {
  none: "no budget set",
  ok: "on pace",
  warn: "above pace",
  over: "over budget",
};

export function BurnWidget({
  projectKey,
  canEdit,
}: {
  projectKey: string;
  canEdit: boolean;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["burn", projectKey],
    queryFn: () => getBurn(projectKey),
    refetchInterval: 60_000,
  });
  const [editing, setEditing] = useState(false);

  const setBudget = useMutation({
    mutationFn: ({ amount }: { amount: number | null }) =>
      setProjectBudget(projectKey, { budget_cents: amount }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["burn", projectKey] });
      setEditing(false);
    },
  });

  if (!q.data) return null;
  const { spent_cents, budget_cents, currency, elapsed_fraction, status } = q.data;
  const spent = fmtMoneyCents(spent_cents, currency);
  const budget = budget_cents != null ? fmtMoneyCents(budget_cents, currency) : "—";
  const pct =
    budget_cents != null && budget_cents > 0
      ? Math.min(120, (spent_cents / budget_cents) * 100)
      : 0;
  const idealPct = elapsed_fraction * 100;

  return (
    <section className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
          burn rate · {STATUS_LABEL[status] ?? status}
        </h2>
        {canEdit && (
          <button
            type="button"
            onClick={() => setEditing((v) => !v)}
            className="text-chrome-dim hover:text-chrome"
            aria-label="edit budget"
          >
            {editing ? <X size={12} /> : <Pencil size={12} />}
          </button>
        )}
      </div>

      <div className="mb-1 flex items-baseline justify-between">
        <span className="mono text-2xl text-chrome">{spent}</span>
        <span className="mono text-xs text-chrome-dim">of {budget}</span>
      </div>

      <div className="relative h-2 overflow-hidden rounded-full bg-ink">
        {budget_cents != null && budget_cents > 0 && (
          <>
            <div
              className="absolute inset-y-0 left-0"
              style={{
                width: `${pct}%`,
                background: STATUS_COLOR[status] ?? "#7c5cff",
              }}
            />
            <div
              className="absolute inset-y-0 w-px bg-white/30"
              style={{ left: `${idealPct}%` }}
              title="pace ideal"
            />
          </>
        )}
      </div>
      <div className="mono mt-1 flex items-center justify-between text-[10px] text-chrome-dim">
        <span>{Math.round(elapsed_fraction * 100)}% of month elapsed</span>
        {budget_cents != null && (
          <span style={{ color: STATUS_COLOR[status] }}>{Math.round(pct)}%</span>
        )}
      </div>

      {editing && (
        <EditBudgetForm
          currency={currency}
          current={budget_cents}
          onSave={(amount) => setBudget.mutate({ amount })}
          busy={setBudget.isPending}
        />
      )}
    </section>
  );
}

function EditBudgetForm({
  currency,
  current,
  onSave,
  busy,
}: {
  currency: string;
  current: number | null;
  onSave: (amount: number | null) => void;
  busy: boolean;
}) {
  const [amount, setAmount] = useState(
    current != null ? (current / 100).toString() : "",
  );
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const parsed = amount.trim() === "" ? null : Math.round(Number(amount) * 100);
        if (parsed != null && (!Number.isFinite(parsed) || parsed < 0)) {
          alert("budget must be a positive number");
          return;
        }
        onSave(parsed);
      }}
      className="mt-3 flex items-center gap-1 border-t border-white/10 pt-3"
    >
      <span className="mono text-[11px] text-chrome-dim">{currency}</span>
      <input
        type="number"
        min={0}
        step="0.01"
        value={amount}
        onChange={(e) => setAmount(e.target.value)}
        placeholder="0.00"
        className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
      />
      <button
        type="submit"
        disabled={busy}
        className="mono rounded bg-accent px-2 py-1 text-[11px] text-accent-fg disabled:opacity-50"
      >
        save
      </button>
      <button
        type="button"
        onClick={() => onSave(null)}
        disabled={busy}
        className="mono rounded border border-white/10 px-2 py-1 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
        title="clear budget"
      >
        clear
      </button>
    </form>
  );
}
