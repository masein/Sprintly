"use client";

// Retro page. Four columns. Anonymous toggle on the per-column composer.
// Voting via thumbs-up button. "Promote to task" on action items.

import { useState } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ThumbsUp, Trash2, ArrowRight, Lock, Sparkles, X,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Markdown } from "@/components/Markdown";
import {
  closeRetro,
  createNote,
  deleteNote,
  getRetro,
  getSprint,
  promoteNote,
  unvoteOnNote,
  voteOnNote,
  type RetroNote,
} from "@/lib/sprints";
import { me } from "@/lib/auth-bundle";
import type { ApiError } from "@/lib/api";

const COLUMNS: { kind: RetroNote["column_kind"]; label: string; hint: string }[] = [
  { kind: "went_well", label: "went well", hint: "what worked" },
  { kind: "went_poorly", label: "went poorly", hint: "what didn't" },
  { kind: "action_item", label: "action items", hint: "what we'll change" },
  { kind: "kudos", label: "kudos", hint: "who made it happen" },
];

export default function RetroPage() {
  const router = useRouter();
  const params = useParams<{ id: string }>();
  const sprintId = params?.id ?? "";
  const qc = useQueryClient();

  const sprintQ = useQuery({
    queryKey: ["sprint", sprintId],
    queryFn: () => getSprint(sprintId),
    enabled: !!sprintId,
  });
  const retroQ = useQuery({
    queryKey: ["retro", sprintId],
    queryFn: () => getRetro(sprintId),
    enabled: !!sprintId,
  });
  const meQ = useQuery({ queryKey: ["me"], queryFn: () => me() });

  const close = useMutation({
    mutationFn: () => closeRetro(retroQ.data!.id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["retro", sprintId] });
      qc.invalidateQueries({ queryKey: ["sprint", sprintId] });
    },
    onError: (e) => alert((e as unknown as ApiError).message),
  });

  if (retroQ.error) {
    const e = retroQ.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
    if (e.status === 404) {
      return (
        <AppShell>
          <div className="mono rounded border border-white/10 bg-ink-subtle p-4 text-sm text-chrome-dim">
            no retro yet — complete the sprint to open one
          </div>
        </AppShell>
      );
    }
  }

  if (!retroQ.data || !sprintQ.data) {
    return (
      <AppShell>
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      </AppShell>
    );
  }

  const retro = retroQ.data;
  const sprint = sprintQ.data;
  const isClosed = retro.state === "closed";

  return (
    <AppShell>
      <div className="mb-4 flex items-center gap-3">
        <Link
          href={`/sprints/${sprintId}`}
          className="mono text-xs text-chrome-dim hover:text-chrome"
        >
          ← {sprint.name}
        </Link>
        <span
          className={`mono inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${
            isClosed
              ? "border-white/10 text-chrome-dim"
              : "border-accent bg-accent/10 text-accent"
          }`}
        >
          {isClosed ? <Lock size={9} /> : <Sparkles size={9} />} retro · {retro.state}
        </span>
      </div>

      <header className="mb-6">
        <h1 className="text-3xl font-semibold">Retrospective.</h1>
        <p className="mt-1 text-sm text-chrome-dim">
          {isClosed
            ? "Locked. The markdown summary below is what gets shared."
            : "Drop a note. Vote on others. Anonymity is fine — pick the box."}
        </p>
      </header>

      {isClosed && sprint.summary_md && (
        <section className="mb-6 rounded-lg border border-white/10 bg-ink-subtle p-4">
          <div className="mono mb-2 flex items-center justify-between text-xs uppercase tracking-widest text-chrome-dim">
            <span>summary · markdown</span>
            <button
              type="button"
              onClick={() => {
                navigator.clipboard.writeText(sprint.summary_md ?? "");
              }}
              className="text-chrome-dim hover:text-chrome"
            >
              copy
            </button>
          </div>
          <Markdown>{sprint.summary_md}</Markdown>
        </section>
      )}

      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
        {COLUMNS.map((c) => (
          <Column
            key={c.kind}
            label={c.label}
            hint={c.hint}
            kind={c.kind}
            notes={retro.notes[c.kind] ?? []}
            retroId={retro.id}
            sprintId={sprintId}
            readonly={isClosed}
            currentUserId={meQ.data?.id}
            isAdmin={meQ.data?.role === "admin"}
          />
        ))}
      </div>

      {!isClosed && (
        <div className="mt-6 flex justify-end">
          <button
            type="button"
            onClick={() => {
              if (!confirm("Close the retro? Generates the markdown summary and locks notes.")) return;
              close.mutate();
            }}
            disabled={close.isPending}
            className="mono inline-flex items-center gap-2 rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
          >
            <Lock size={14} /> {close.isPending ? "closing…" : "close + write summary"}
          </button>
        </div>
      )}
    </AppShell>
  );
}

function Column({
  label,
  hint,
  kind,
  notes,
  retroId,
  sprintId,
  readonly,
  currentUserId,
  isAdmin,
}: {
  label: string;
  hint: string;
  kind: RetroNote["column_kind"];
  notes: RetroNote[];
  retroId: string;
  sprintId: string;
  readonly: boolean;
  currentUserId: string | undefined;
  isAdmin: boolean | undefined;
}) {
  const qc = useQueryClient();
  const [body, setBody] = useState("");
  const [anonymous, setAnonymous] = useState(false);

  const add = useMutation({
    mutationFn: () => createNote(retroId, { column_kind: kind, body, anonymous }),
    onSuccess: () => {
      setBody("");
      qc.invalidateQueries({ queryKey: ["retro", sprintId] });
    },
  });

  return (
    <section className="rounded-lg border border-white/10 bg-ink-subtle p-3">
      <header className="mb-2 flex items-center justify-between">
        <h2 className="mono text-xs uppercase tracking-widest text-chrome">{label}</h2>
        <span className="mono text-[10px] text-chrome-dim">{hint}</span>
      </header>

      <ul className="space-y-2">
        {notes.map((n) => (
          <NoteCard
            key={n.id}
            note={n}
            sprintId={sprintId}
            canEdit={
              isAdmin ||
              (!n.anonymous && n.author_handle != null && currentUserId != null
                ? false // we don't have user_id of author on the DTO; rely on author_handle for display
                : false)
            }
            canDelete={isAdmin === true}
            allowPromote={kind === "action_item" && !readonly}
          />
        ))}
      </ul>

      {!readonly && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (body.trim()) add.mutate();
          }}
          className="mt-2 space-y-1"
        >
          <textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            rows={2}
            placeholder={`add to ${label}…`}
            className="block w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          <div className="flex items-center justify-between">
            <label className="mono flex items-center gap-1 text-[10px] text-chrome-dim">
              <input
                type="checkbox"
                checked={anonymous}
                onChange={(e) => setAnonymous(e.target.checked)}
              />
              anonymous
            </label>
            <button
              type="submit"
              disabled={!body.trim() || add.isPending}
              className="mono rounded bg-accent px-2 py-1 text-[10px] text-accent-fg disabled:opacity-50"
            >
              add
            </button>
          </div>
        </form>
      )}
    </section>
  );
}

function NoteCard({
  note,
  sprintId,
  canDelete,
  allowPromote,
}: {
  note: RetroNote;
  sprintId: string;
  canEdit?: boolean;
  canDelete: boolean;
  allowPromote: boolean;
}) {
  const qc = useQueryClient();
  const toggleVote = useMutation({
    mutationFn: () =>
      note.you_voted ? unvoteOnNote(note.id) : voteOnNote(note.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["retro", sprintId] }),
  });
  const promote = useMutation({
    mutationFn: () => promoteNote(note.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["retro", sprintId] }),
    onError: (e) => alert((e as unknown as ApiError).message),
  });
  const del = useMutation({
    mutationFn: () => deleteNote(note.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["retro", sprintId] }),
  });

  return (
    <li className="rounded border border-white/10 bg-ink p-2 text-sm">
      <div className="whitespace-pre-wrap text-chrome">{note.body}</div>
      <div className="mt-1.5 flex items-center gap-2 text-[10px]">
        <span className="mono text-chrome-dim">
          {note.anonymous ? "anonymous" : note.author_handle ? `@${note.author_handle}` : "—"}
        </span>
        <button
          type="button"
          onClick={() => toggleVote.mutate()}
          className={`mono inline-flex items-center gap-0.5 rounded border px-1 py-0.5 text-[10px] transition ${
            note.you_voted
              ? "border-accent bg-accent/10 text-accent"
              : "border-white/10 text-chrome-dim hover:border-white/20"
          }`}
        >
          <ThumbsUp size={9} /> {note.vote_count}
        </button>
        {allowPromote && (
          note.promoted_task_key ? (
            <Link
              href={`/tasks/${note.promoted_task_key}`}
              className="mono inline-flex items-center gap-0.5 text-[10px] text-accent hover:underline"
            >
              <ArrowRight size={9} /> {note.promoted_task_key}
            </Link>
          ) : (
            <button
              type="button"
              onClick={() => promote.mutate()}
              disabled={promote.isPending}
              className="mono inline-flex items-center gap-0.5 rounded border border-white/10 px-1 py-0.5 text-[10px] text-chrome-dim hover:border-white/20 hover:text-chrome disabled:opacity-50"
            >
              <ArrowRight size={9} /> promote to task
            </button>
          )
        )}
        {canDelete && (
          <button
            type="button"
            onClick={() => del.mutate()}
            className="ml-auto text-chrome-dim hover:text-red-300"
            aria-label="Delete note"
          >
            <Trash2 size={10} />
          </button>
        )}
      </div>
    </li>
  );
}
