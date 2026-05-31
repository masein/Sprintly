"use client";

// Threaded comments (one level). Inline reactions. Author/admin can edit
// and soft-delete. Other users see read-only.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MoreHorizontal, Smile, Reply, Trash2, Pencil, X } from "lucide-react";
import {
  addReaction,
  createComment,
  deleteComment,
  editComment,
  listComments,
  removeReaction,
  type Comment,
} from "@/lib/task-detail";
import { me, type Me } from "@/lib/auth-bundle";
import { Markdown } from "./Markdown";

const QUICK_EMOJI = ["👍", "🎉", "🚀", "❤️", "👀", "🙌"];

export function CommentThread({ taskKey }: { taskKey: string }) {
  const qc = useQueryClient();
  const comments = useQuery({
    queryKey: ["comments", taskKey],
    queryFn: () => listComments(taskKey),
  });
  const currentUser = useQuery({ queryKey: ["me"], queryFn: () => me() });

  const create = useMutation({
    mutationFn: (input: { body: string; parent_comment_id?: string }) =>
      createComment(taskKey, input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["comments", taskKey] }),
  });

  if (comments.isLoading) {
    return <div className="mono text-xs text-chrome-dim">compiling vibes…</div>;
  }

  // Group: top-level first; each carries its (one-level) replies.
  const top = (comments.data ?? []).filter((c) => !c.parent_comment_id);
  const repliesByParent = new Map<string, Comment[]>();
  for (const c of comments.data ?? []) {
    if (c.parent_comment_id) {
      const list = repliesByParent.get(c.parent_comment_id) ?? [];
      list.push(c);
      repliesByParent.set(c.parent_comment_id, list);
    }
  }

  return (
    <section className="space-y-4">
      <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
        comments ({comments.data?.length ?? 0})
      </h2>

      <NewCommentForm
        onSubmit={async (body) => {
          await create.mutateAsync({ body });
        }}
        placeholder="leave a comment — markdown supported"
      />

      <div className="space-y-4">
        {top.length === 0 && (
          <div className="mono rounded border border-dashed border-white/10 p-4 text-center text-xs text-chrome-dim">
            no comments yet — be the first
          </div>
        )}
        {top.map((c) => (
          <CommentItem
            key={c.id}
            comment={c}
            taskKey={taskKey}
            replies={repliesByParent.get(c.id) ?? []}
            currentUser={currentUser.data}
          />
        ))}
      </div>
    </section>
  );
}

function CommentItem({
  comment,
  taskKey,
  replies,
  currentUser,
}: {
  comment: Comment;
  taskKey: string;
  replies: Comment[];
  currentUser: Me | undefined;
}) {
  const qc = useQueryClient();
  const isMine =
    !!currentUser && comment.author_id === currentUser.id;
  const isAdmin = currentUser?.role === "admin";
  const canEdit = isMine;
  const canDelete = isMine || isAdmin;

  const [editing, setEditing] = useState(false);
  const [replying, setReplying] = useState(false);

  const edit = useMutation({
    mutationFn: (body: string) => editComment(comment.id, body),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["comments", taskKey] }),
  });
  const del = useMutation({
    mutationFn: () => deleteComment(comment.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["comments", taskKey] }),
  });
  const react = useMutation({
    mutationFn: (emoji: string) => addReaction({ comment_id: comment.id, emoji }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["comments", taskKey] }),
  });
  const reply = useMutation({
    mutationFn: (body: string) =>
      createComment(taskKey, { body, parent_comment_id: comment.id }),
    onSuccess: () => {
      setReplying(false);
      qc.invalidateQueries({ queryKey: ["comments", taskKey] });
    },
  });

  return (
    <article className="rounded-lg border border-white/10 bg-ink-subtle p-3">
      <header className="mb-2 flex items-center gap-2">
        <span className="mono text-xs text-chrome">
          @{comment.author_handle ?? "?"}
        </span>
        <span className="mono text-[10px] text-chrome-dim">
          {fmtTime(comment.created_at)}
          {comment.edited_at ? " · edited" : ""}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <QuickEmoji onPick={(e) => react.mutate(e)} />
          {!comment.parent_comment_id && (
            <IconButton
              label="Reply"
              onClick={() => setReplying((v) => !v)}
            >
              <Reply size={13} />
            </IconButton>
          )}
          {canEdit && (
            <IconButton label="Edit" onClick={() => setEditing((v) => !v)}>
              <Pencil size={13} />
            </IconButton>
          )}
          {canDelete && (
            <IconButton
              label="Delete"
              onClick={() => del.mutate()}
              destructive
            >
              <Trash2 size={13} />
            </IconButton>
          )}
        </div>
      </header>

      {editing ? (
        <EditCommentForm
          initial={comment.body}
          onCancel={() => setEditing(false)}
          onSave={async (next) => {
            await edit.mutateAsync(next);
            setEditing(false);
          }}
        />
      ) : (
        <Markdown>{comment.body}</Markdown>
      )}

      {comment.reactions.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {comment.reactions.map((g) => (
            <button
              type="button"
              key={g.emoji}
              onClick={() => react.mutate(g.emoji)}
              className={`mono rounded border px-1.5 py-0.5 text-xs transition ${
                g.user_reacted
                  ? "border-accent bg-accent/10"
                  : "border-white/10 text-chrome-dim hover:border-white/20"
              }`}
            >
              {g.emoji} {g.count}
            </button>
          ))}
        </div>
      )}

      {replies.length > 0 && (
        <div className="mt-3 space-y-3 border-l border-white/10 pl-3">
          {replies.map((r) => (
            <CommentItem
              key={r.id}
              comment={r}
              taskKey={taskKey}
              replies={[]}
              currentUser={currentUser}
            />
          ))}
        </div>
      )}

      {replying && (
        <div className="mt-3">
          <NewCommentForm
            onSubmit={async (body) => {
              await reply.mutateAsync(body);
            }}
            placeholder="reply…"
            onCancel={() => setReplying(false)}
          />
        </div>
      )}
    </article>
  );
}

function NewCommentForm({
  onSubmit,
  placeholder,
  onCancel,
}: {
  onSubmit: (body: string) => Promise<void>;
  placeholder: string;
  onCancel?: () => void;
}) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        if (!body.trim()) return;
        setBusy(true);
        try {
          await onSubmit(body);
          setBody("");
        } finally {
          setBusy(false);
        }
      }}
      className="space-y-2"
    >
      <textarea
        value={body}
        onChange={(e) => setBody(e.target.value)}
        placeholder={placeholder}
        rows={3}
        className="block w-full rounded border border-white/10 bg-ink px-3 py-2 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <div className="flex items-center justify-between">
        <span className="mono text-[10px] text-chrome-dim">
          markdown · `code`, **bold**, etc.
        </span>
        <div className="flex items-center gap-2">
          {onCancel && (
            <button
              type="button"
              onClick={onCancel}
              className="mono text-xs text-chrome-dim hover:text-chrome"
            >
              :q
            </button>
          )}
          <button
            type="submit"
            disabled={!body.trim() || busy}
            className="mono rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50"
          >
            {busy ? "…" : "$ commit"}
          </button>
        </div>
      </div>
    </form>
  );
}

function EditCommentForm({
  initial,
  onSave,
  onCancel,
}: {
  initial: string;
  onSave: (body: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [body, setBody] = useState(initial);
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        await onSave(body);
      }}
      className="space-y-2"
    >
      <textarea
        value={body}
        onChange={(e) => setBody(e.target.value)}
        rows={4}
        className="block w-full rounded border border-white/10 bg-ink px-3 py-2 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <div className="flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="mono text-xs text-chrome-dim hover:text-chrome"
        >
          cancel
        </button>
        <button
          type="submit"
          disabled={!body.trim()}
          className="mono rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50"
        >
          save
        </button>
      </div>
    </form>
  );
}

function QuickEmoji({ onPick }: { onPick: (e: string) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <IconButton label="React" onClick={() => setOpen((v) => !v)}>
        <Smile size={13} />
      </IconButton>
      {open && (
        <div className="absolute right-0 top-full z-10 mt-1 flex gap-0.5 rounded border border-white/10 bg-ink p-1 shadow-xl">
          {QUICK_EMOJI.map((e) => (
            <button
              type="button"
              key={e}
              onClick={() => {
                onPick(e);
                setOpen(false);
              }}
              className="rounded px-1.5 py-1 text-base hover:bg-white/5"
            >
              {e}
            </button>
          ))}
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="rounded px-1 text-chrome-dim hover:text-chrome"
            aria-label="close"
          >
            <X size={12} />
          </button>
        </div>
      )}
    </div>
  );
}

function IconButton({
  children,
  label,
  destructive,
  onClick,
}: {
  children: React.ReactNode;
  label: string;
  destructive?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={`rounded p-1 transition ${
        destructive
          ? "text-chrome-dim hover:bg-red-500/10 hover:text-red-200"
          : "text-chrome-dim hover:bg-white/5 hover:text-chrome"
      }`}
    >
      {children}
    </button>
  );
}

function fmtTime(iso: string): string {
  const d = new Date(iso);
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return `${Math.floor(diff)}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return d.toISOString().slice(0, 10);
}
