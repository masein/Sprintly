"use client";

// Watchers: who gets told when this task changes. Anyone can watch or stop
// watching themselves; the team (leads and contributors) can also add other
// project members and take them off again (QA report 6: "enable users to add
// and manage additional team members as watchers").

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, EyeOff, UserPlus, X } from "lucide-react";
import { addWatcher, listWatchers, removeWatcher } from "@/lib/task-detail";
import { me } from "@/lib/auth-bundle";
import { listMembers } from "@/lib/projects";
import type { ApiError } from "@/lib/api";
import { Avatar } from "./Avatar";

export function Watchers({
  taskKey,
  projectKey,
  canManage,
}: {
  taskKey: string;
  projectKey: string;
  /** May add and remove *other* people. Watching yourself needs nothing. */
  canManage: boolean;
}) {
  const qc = useQueryClient();
  const w = useQuery({ queryKey: ["watchers", taskKey], queryFn: () => listWatchers(taskKey) });
  const user = useQuery({ queryKey: ["me"], queryFn: () => me() });
  const members = useQuery({
    queryKey: ["project-members", projectKey],
    queryFn: () => listMembers(projectKey),
    enabled: canManage,
    staleTime: 60_000,
  });
  const [error, setError] = useState<string | null>(null);

  const watching = useMemo(() => w.data ?? [], [w.data]);
  const isWatching = !!user.data && watching.some((x) => x.user_id === user.data!.id);
  const refresh = () => qc.invalidateQueries({ queryKey: ["watchers", taskKey] });
  const fail = (e: unknown) => setError((e as ApiError).message ?? "that didn't work");

  const add = useMutation({
    mutationFn: (userId: string) => addWatcher(taskKey, userId),
    onSuccess: () => {
      setError(null);
      void refresh();
    },
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: (userId: string) => removeWatcher(taskKey, userId),
    onSuccess: () => {
      setError(null);
      void refresh();
    },
    onError: fail,
  });

  const candidates = useMemo(() => {
    const taken = new Set(watching.map((x) => x.user_id));
    return (members.data ?? []).filter((m) => !taken.has(m.user_id));
  }, [members.data, watching]);

  return (
    <section className="space-y-2" aria-label="watchers">
      <h2 className="mono flex items-center justify-between text-xs uppercase tracking-widest text-chrome-dim">
        <span>watchers ({watching.length})</span>
        {user.data && (
          <button
            type="button"
            onClick={() => (isWatching ? remove.mutate(user.data!.id) : add.mutate(user.data!.id))}
            className="mono flex items-center gap-1 rounded border border-white/10 px-1.5 py-0.5 text-[10px] normal-case tracking-normal text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            {isWatching ? (
              <><EyeOff size={11} /> stop watching</>
            ) : (
              <><Eye size={11} /> watch</>
            )}
          </button>
        )}
      </h2>
      <ul className="space-y-1">
        {watching.map((x) => {
          const self = x.user_id === user.data?.id;
          return (
            <li key={x.user_id} className="group mono flex items-center gap-2 text-xs">
              <Avatar
                size={18}
                user={{
                  userId: x.user_id,
                  displayName: x.display_name,
                  handle: x.handle,
                  avatarUrl: x.avatar_url,
                  avatarStyle: x.avatar_style,
                  avatarSeed: x.avatar_seed,
                }}
              />
              <span className="text-chrome">@{x.handle}</span>
              <span className="min-w-0 truncate text-chrome-dim">{x.display_name}</span>
              {(canManage || self) && (
                <button
                  type="button"
                  onClick={() => remove.mutate(x.user_id)}
                  aria-label={`remove @${x.handle} from watchers`}
                  title={self ? "stop watching" : `remove @${x.handle}`}
                  className="ml-auto text-chrome-dim opacity-60 hover:text-red-300 group-hover:opacity-100"
                >
                  <X size={12} />
                </button>
              )}
            </li>
          );
        })}
        {w.data?.length === 0 && (
          <li className="mono text-[11px] text-chrome-dim">no watchers</li>
        )}
      </ul>
      {canManage && (
        <AddWatcher
          candidates={candidates}
          loading={members.isLoading}
          onPick={(id) => add.mutate(id)}
        />
      )}
      {error && <div className="mono text-[11px] text-red-300">{error}</div>}
    </section>
  );
}

function AddWatcher({
  candidates,
  loading,
  onPick,
}: {
  candidates: { user_id: string; handle: string; display_name: string; avatar_url: string | null; avatar_style: string | null; avatar_seed: string | null }[];
  loading: boolean;
  onPick: (userId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const box = useRef<HTMLDivElement>(null);
  const matches = useMemo(() => {
    const needle = q.trim().toLowerCase().replace(/^@/, "");
    if (!needle) return candidates;
    return candidates.filter(
      (m) =>
        m.handle.toLowerCase().includes(needle) || m.display_name.toLowerCase().includes(needle),
    );
  }, [candidates, q]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mono flex items-center gap-1 text-[11px] text-chrome-dim hover:text-chrome"
      >
        <UserPlus size={11} /> add a watcher
      </button>
    );
  }
  return (
    <div ref={box} className="space-y-1 rounded border border-white/10 bg-ink p-1.5">
      <input
        autoFocus
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            setOpen(false);
            setQ("");
          } else if (e.key === "Enter" && matches[0]) {
            e.preventDefault();
            onPick(matches[0].user_id);
            setQ("");
          }
        }}
        placeholder="find a teammate…"
        aria-label="find a teammate to add as a watcher"
        className="mono w-full rounded border border-white/10 bg-ink-subtle px-1.5 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
      />
      <ul className="max-h-40 overflow-y-auto" role="listbox" aria-label="teammates">
        {matches.map((m) => (
          <li key={m.user_id}>
            <button
              type="button"
              role="option"
              aria-selected={false}
              onClick={() => {
                onPick(m.user_id);
                setQ("");
              }}
              className="mono flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs text-chrome-dim hover:bg-white/5 hover:text-chrome"
            >
              <Avatar
                size={16}
                user={{
                  userId: m.user_id,
                  displayName: m.display_name,
                  handle: m.handle,
                  avatarUrl: m.avatar_url,
                  avatarStyle: m.avatar_style,
                  avatarSeed: m.avatar_seed,
                }}
              />
              <span className="text-chrome">@{m.handle}</span>
              <span className="truncate">{m.display_name}</span>
            </button>
          </li>
        ))}
        {!loading && matches.length === 0 && (
          <li className="mono px-1 py-1 text-[11px] text-chrome-dim">
            {candidates.length === 0 ? "everyone's already watching" : "nobody by that name"}
          </li>
        )}
      </ul>
    </div>
  );
}
