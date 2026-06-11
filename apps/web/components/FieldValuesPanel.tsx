"use client";

// Task-detail sidebar panel: every custom field the project defines, with
// this task's value where set. Editors match the field type — text, number,
// date inputs; select gets the option list.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import {
  clearTaskFieldValue,
  listTaskFieldValues,
  setTaskFieldValue,
  type TaskFieldValue,
} from "@/lib/fields";
import type { ApiError } from "@/lib/api";

export function FieldValuesPanel({
  taskKey,
  canEdit,
}: {
  taskKey: string;
  canEdit: boolean;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["task-fields", taskKey],
    queryFn: () => listTaskFieldValues(taskKey),
    retry: false,
  });
  const invalidate = () =>
    qc.invalidateQueries({ queryKey: ["task-fields", taskKey] });

  const [error, setError] = useState<string | null>(null);

  const set = useMutation({
    mutationFn: (v: { fieldId: string; value: string }) =>
      setTaskFieldValue(taskKey, v.fieldId, v.value),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const clear = useMutation({
    mutationFn: (fieldId: string) => clearTaskFieldValue(taskKey, fieldId),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
  });

  const fields = q.data ?? [];
  // A project with no schema shows nothing — the panel earns its pixels only
  // when there's something to say.
  if (fields.length === 0) return null;

  return (
    <section className="space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
        fields
      </h2>
      {fields.map((f) => (
        <FieldRow
          key={f.field_id}
          field={f}
          canEdit={canEdit}
          onSet={(value) => set.mutate({ fieldId: f.field_id, value })}
          onClear={() => clear.mutate(f.field_id)}
        />
      ))}
      {error && <div className="mono text-[11px] text-red-300">{error}</div>}
    </section>
  );
}

function FieldRow({
  field,
  canEdit,
  onSet,
  onClear,
}: {
  field: TaskFieldValue;
  canEdit: boolean;
  onSet: (value: string) => void;
  onClear: () => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);

  const inputType =
    field.type === "number" ? "number" : field.type === "date" ? "date" : "text";

  if (!canEdit) {
    return (
      <div className="flex items-center justify-between gap-3">
        <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
          {field.name}
        </span>
        <span className="mono text-xs text-chrome">{field.value ?? "—"}</span>
      </div>
    );
  }

  return (
    <div className="flex items-center justify-between gap-3">
      <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
        {field.name}
      </span>
      <span className="flex items-center gap-1">
        {field.type === "select" ? (
          <select
            value={field.value ?? ""}
            onChange={(e) => {
              if (e.target.value) onSet(e.target.value);
            }}
            aria-label={field.name}
            className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome"
          >
            <option value="">—</option>
            {field.options.map((o) => (
              <option key={o} value={o}>{o}</option>
            ))}
          </select>
        ) : (
          <input
            type={inputType}
            step={field.type === "number" ? "any" : undefined}
            value={draft ?? field.value ?? ""}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => {
              if (draft !== null && draft.trim() && draft !== field.value) {
                onSet(draft.trim());
              }
              setDraft(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            }}
            placeholder="—"
            aria-label={field.name}
            className="mono w-28 rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome focus:border-accent focus:outline-none"
          />
        )}
        {field.value !== null && (
          <button
            type="button"
            onClick={onClear}
            aria-label={`clear ${field.name}`}
            className="text-chrome-dim hover:text-chrome"
          >
            <X size={11} />
          </button>
        )}
      </span>
    </div>
  );
}
