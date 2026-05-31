"use client";

// Per-task timer surface. Big start/stop button + manual-entry form.
//
// Logic:
//   • If no log is running anywhere → "Start timer" mints a new one on this task.
//   • If THIS task is the running one → "Stop" closes it.
//   • If ANOTHER task is running → disabled button hints "stop X first".

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, Square, Plus, ChevronDown } from "lucide-react";
import {
  createManualLog,
  currentTimer,
  deleteLog,
  fmtMinutes,
  listTaskLogs,
  startTimer,
  stopTimer,
  type TimeLog,
} from "@/lib/timetracking";
import type { ApiError } from "@/lib/api";

export function TaskTimer({ taskKey }: { taskKey: string }) {
  const qc = useQueryClient();
  const timerQ = useQuery({
    queryKey: ["me-timer"],
    queryFn: () => currentTimer(),
  });
  const logsQ = useQuery({
    queryKey: ["task-logs", taskKey],
    queryFn: () => listTaskLogs(taskKey),
  });

  const running = timerQ.data?.running ?? null;
  const isThisTask = running?.task_key === taskKey;
  const otherKey = running && !isThisTask ? running.task_key : null;

  const start = useMutation({
    mutationFn: () => startTimer(taskKey),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["me-timer"] });
      qc.invalidateQueries({ queryKey: ["task-logs", taskKey] });
    },
    onError: (e) => alert((e as ApiError).message),
  });
  const stop = useMutation({
    mutationFn: () => stopTimer(),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["me-timer"] });
      qc.invalidateQueries({ queryKey: ["task-logs", taskKey] });
    },
  });
  const del = useMutation({
    mutationFn: (id: string) => deleteLog(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["task-logs", taskKey] }),
  });

  const [manualOpen, setManualOpen] = useState(false);

  return (
    <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
      <div className="flex items-center gap-3">
        {isThisTask ? (
          <button
            type="button"
            onClick={() => stop.mutate()}
            disabled={stop.isPending}
            className="mono inline-flex items-center gap-2 rounded bg-red-500/20 px-3 py-2 text-sm text-red-200 hover:bg-red-500/30 disabled:opacity-50"
          >
            <Square size={14} /> stop timer
          </button>
        ) : (
          <button
            type="button"
            onClick={() => start.mutate()}
            disabled={!!otherKey || start.isPending}
            title={otherKey ? `stop ${otherKey} first` : undefined}
            className="mono inline-flex items-center gap-2 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Play size={14} /> {otherKey ? `running on ${otherKey}` : "start timer"}
          </button>
        )}

        <button
          type="button"
          onClick={() => setManualOpen((v) => !v)}
          className="mono inline-flex items-center gap-1 text-xs text-chrome-dim hover:text-chrome"
        >
          <Plus size={12} /> manual entry
          <ChevronDown size={11} className={manualOpen ? "rotate-180" : ""} />
        </button>
      </div>

      {manualOpen && (
        <ManualEntry
          taskKey={taskKey}
          onSaved={() => {
            setManualOpen(false);
            qc.invalidateQueries({ queryKey: ["task-logs", taskKey] });
          }}
        />
      )}

      <LogList logs={logsQ.data ?? []} onDelete={(id) => del.mutate(id)} />
    </section>
  );
}

function ManualEntry({
  taskKey,
  onSaved,
}: {
  taskKey: string;
  onSaved: () => void;
}) {
  const [date, setDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [hours, setHours] = useState("0");
  const [minutes, setMinutes] = useState("30");
  const [note, setNote] = useState("");
  const [billable, setBillable] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const mins = parseInt(hours, 10) * 60 + parseInt(minutes, 10);
    if (!Number.isFinite(mins) || mins <= 0) {
      setError("enter a positive duration");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const startedAt = new Date(`${date}T09:00:00`).toISOString();
      await createManualLog(taskKey, {
        started_at: startedAt,
        duration_minutes: mins,
        note: note || undefined,
        billable,
      });
      onSaved();
    } catch (e) {
      setError((e as ApiError).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      onSubmit={submit}
      className="space-y-2 rounded border border-white/10 bg-ink p-3"
    >
      <div className="flex items-center gap-2">
        <input
          type="date"
          value={date}
          onChange={(e) => setDate(e.target.value)}
          className="mono rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
        />
        <input
          type="number"
          min={0}
          value={hours}
          onChange={(e) => setHours(e.target.value)}
          className="mono w-14 rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
          aria-label="hours"
        />
        <span className="mono text-[10px] text-chrome-dim">h</span>
        <input
          type="number"
          min={0}
          max={59}
          value={minutes}
          onChange={(e) => setMinutes(e.target.value)}
          className="mono w-14 rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
          aria-label="minutes"
        />
        <span className="mono text-[10px] text-chrome-dim">m</span>
        <label className="mono ml-auto flex items-center gap-1 text-[11px] text-chrome-dim">
          <input
            type="checkbox"
            checked={billable}
            onChange={(e) => setBillable(e.target.checked)}
          />
          billable
        </label>
      </div>
      <input
        value={note}
        onChange={(e) => setNote(e.target.value)}
        placeholder="note (optional)"
        className="block w-full rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
      />
      {error && (
        <div className="mono text-[11px] text-red-200">{error}</div>
      )}
      <div className="flex items-center justify-end">
        <button
          type="submit"
          disabled={busy}
          className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
        >
          {busy ? "…" : "add log"}
        </button>
      </div>
    </form>
  );
}

function LogList({
  logs,
  onDelete,
}: {
  logs: TimeLog[];
  onDelete: (id: string) => void;
}) {
  if (logs.length === 0) {
    return (
      <div className="mono text-center text-[11px] text-chrome-dim">
        no logs yet
      </div>
    );
  }
  return (
    <ul className="space-y-1 border-t border-white/10 pt-2">
      {logs.map((l) => (
        <li
          key={l.id}
          className="mono flex items-center gap-2 text-xs text-chrome-dim"
        >
          <span className="text-chrome">
            {l.duration_minutes != null ? fmtMinutes(l.duration_minutes) : "running…"}
          </span>
          {!l.billable && (
            <span className="rounded border border-white/10 px-1 py-0.5 text-[9px] uppercase">
              non-billable
            </span>
          )}
          <span className="truncate">{l.note || ""}</span>
          <span className="ml-auto text-[10px]">
            {new Date(l.started_at).toISOString().slice(0, 16).replace("T", " ")}
          </span>
          <button
            type="button"
            onClick={() => onDelete(l.id)}
            className="text-chrome-dim hover:text-red-300"
            aria-label="Delete log"
          >
            ×
          </button>
        </li>
      ))}
    </ul>
  );
}
