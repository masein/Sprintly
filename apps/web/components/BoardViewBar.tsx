"use client";

// Saved-view + swimlane controls above the board. Pick a saved view (restores
// its filter chips + grouping), change the grouping ad-hoc, or save the
// current filter+grouping as a new view (optionally shared with the project).

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Rows3, Save, Trash2, X } from "lucide-react";
import {
  createBoardView,
  deleteBoardView,
  listBoardViews,
  type BoardView,
  type GroupBy,
} from "@/lib/boardViews";
import type { Chip } from "./BoardFilters";
import type { ApiError } from "@/lib/api";

const GROUP_BYS: GroupBy[] = ["none", "assignee", "label", "priority"];

export function BoardViewBar({
  projectKey,
  chips,
  groupBy,
  activeViewId,
  onApplyView,
  onGroupByChange,
}: {
  projectKey: string;
  chips: Chip[];
  groupBy: GroupBy;
  activeViewId: string | null;
  onApplyView: (v: BoardView) => void;
  onGroupByChange: (g: GroupBy) => void;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["board-views", projectKey],
    queryFn: () => listBoardViews(projectKey),
    retry: false,
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: ["board-views", projectKey] });

  const [saving, setSaving] = useState(false);
  const [name, setName] = useState("");
  const [shared, setShared] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: () =>
      createBoardView(projectKey, { name: name.trim(), filter: chips, group_by: groupBy, shared }),
    onSuccess: (v) => {
      setSaving(false);
      setName("");
      setShared(false);
      setError(null);
      invalidate();
      onApplyView(v);
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteBoardView(id),
    onSuccess: invalidate,
  });

  const views = q.data ?? [];
  const active = views.find((v) => v.id === activeViewId) ?? null;

  return (
    <div className="mb-2 flex flex-wrap items-center gap-2">
      <label className="mono flex items-center gap-1 text-[11px] text-chrome-dim">
        view
        <select
          aria-label="saved view"
          value={activeViewId ?? ""}
          onChange={(e) => {
            const v = views.find((x) => x.id === e.target.value);
            if (v) onApplyView(v);
          }}
          className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
        >
          <option value="">{activeViewId ? "(custom)" : "(no view)"}</option>
          {views.map((v) => (
            <option key={v.id} value={v.id}>
              {v.name}
              {v.shared ? " · shared" : ""}
              {v.is_mine ? "" : " (theirs)"}
            </option>
          ))}
        </select>
      </label>

      <label className="mono flex items-center gap-1 text-[11px] text-chrome-dim">
        <Rows3 size={12} /> swimlanes
        <select
          aria-label="swimlane grouping"
          value={groupBy}
          onChange={(e) => onGroupByChange(e.target.value as GroupBy)}
          className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
        >
          {GROUP_BYS.map((g) => (
            <option key={g} value={g}>{g === "none" ? "off" : g}</option>
          ))}
        </select>
      </label>

      {active?.is_mine && (
        <button
          type="button"
          aria-label={`delete view ${active.name}`}
          title="delete this view"
          onClick={() => {
            if (confirm(`Delete the "${active.name}" view?`)) remove.mutate(active.id);
          }}
          className="text-chrome-dim hover:text-red-300"
        >
          <Trash2 size={13} />
        </button>
      )}

      {saving ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) save.mutate();
          }}
          className="flex items-center gap-1"
        >
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={80}
            placeholder="view name"
            className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome focus:border-accent focus:outline-none"
          />
          <label className="mono flex items-center gap-1 text-[11px] text-chrome-dim">
            <input type="checkbox" checked={shared} onChange={(e) => setShared(e.target.checked)} />
            shared
          </label>
          <button
            type="submit"
            disabled={save.isPending || !name.trim()}
            className="mono rounded bg-accent px-1.5 py-0.5 text-[10px] text-accent-fg disabled:opacity-50"
          >
            save
          </button>
          <button
            type="button"
            onClick={() => { setSaving(false); setError(null); }}
            className="text-chrome-dim hover:text-chrome"
            aria-label="cancel"
          >
            <X size={12} />
          </button>
        </form>
      ) : (
        <button
          type="button"
          onClick={() => setSaving(true)}
          className="mono inline-flex items-center gap-1 rounded border border-dashed border-white/10 px-2 py-0.5 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          <Save size={11} /> save view
        </button>
      )}

      {error && <span className="mono text-[11px] text-red-300">{error}</span>}
    </div>
  );
}
