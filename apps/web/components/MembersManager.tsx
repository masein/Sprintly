"use client";

// Manage a project's members: list who's on it, add an existing user by handle
// or email (typeahead), change a role, or remove someone. Opened from the
// project header. Mutations are lead-only; everyone else sees a read-only list.
// The backend is the source of truth for the rules (e.g. you can't remove the
// last lead) — we surface its errors rather than second-guessing them here.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { UserPlus, X } from "lucide-react";
import { Avatar } from "./Avatar";
import {
  addMember,
  changeMemberRole,
  listMembers,
  removeMember,
  type Member,
} from "@/lib/projects";
import { search } from "@/lib/search";
import { me } from "@/lib/auth-bundle";
import type { ApiError } from "@/lib/api";

const ROLES: Member["role"][] = ["lead", "contributor", "watcher"];

export function MembersManager({
  projectKey,
  canManage,
  onClose,
}: {
  projectKey: string;
  canManage: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const meQ = useQuery({ queryKey: ["me"], queryFn: () => me(), staleTime: 60_000 });
  const currentUserId = meQ.data?.id;
  const membersQ = useQuery({
    queryKey: ["project-members", projectKey],
    queryFn: () => listMembers(projectKey),
    retry: false,
  });
  const invalidate = () =>
    qc.invalidateQueries({ queryKey: ["project-members", projectKey] });

  const [error, setError] = useState<string | null>(null);
  const [addRole, setAddRole] = useState<Member["role"]>("contributor");
  const [q, setQ] = useState("");

  const members = membersQ.data ?? [];
  const memberIds = new Set(members.map((m) => m.user_id));

  const hitsQ = useQuery({
    queryKey: ["user-search", q],
    queryFn: () => search(q, 6),
    enabled: canManage && q.trim().length >= 2,
    staleTime: 5_000,
  });
  // Don't offer people who are already on the project.
  const candidates = (hitsQ.data?.users ?? []).filter((u) => !memberIds.has(u.id));

  const add = useMutation({
    mutationFn: (userId: string) => addMember(projectKey, { user_id: userId, role: addRole }),
    onSuccess: () => {
      setQ("");
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "couldn't add them"),
  });
  const changeRole = useMutation({
    mutationFn: (v: { userId: string; role: Member["role"] }) =>
      changeMemberRole(projectKey, v.userId, v.role),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "couldn't change the role"),
  });
  const remove = useMutation({
    mutationFn: (userId: string) => removeMember(projectKey, userId),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "couldn't remove them"),
  });

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="max-h-[90vh] w-full max-w-md space-y-4 overflow-y-auto rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              {projectKey} · members
            </div>
            <h2 className="text-xl font-semibold">Who&apos;s on this project</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        {error && (
          <div
            role="alert"
            className="mono rounded border border-red-500/30 bg-red-500/10 px-2 py-1.5 text-[11px] text-red-200"
          >
            {error}
          </div>
        )}

        <ul className="space-y-1">
          {members.map((m) => (
            <li
              key={m.user_id}
              className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5"
            >
              <Avatar
                size={22}
                user={{
                  userId: m.user_id,
                  displayName: m.display_name,
                  handle: m.handle,
                  avatarUrl: m.avatar_url,
                  avatarStyle: m.avatar_style,
                  avatarSeed: m.avatar_seed,
                }}
              />
              <div className="min-w-0">
                <div className="mono truncate text-xs text-chrome">
                  @{m.handle}
                  {m.user_id === currentUserId && (
                    <span className="text-chrome-dim"> · you</span>
                  )}
                </div>
                <div className="truncate text-[11px] text-chrome-dim">{m.display_name}</div>
              </div>

              <div className="ml-auto flex items-center gap-1">
                {canManage ? (
                  <>
                    <label className="sr-only" htmlFor={`role-${m.user_id}`}>
                      role for @{m.handle}
                    </label>
                    <select
                      id={`role-${m.user_id}`}
                      value={m.role}
                      disabled={changeRole.isPending}
                      onChange={(e) =>
                        changeRole.mutate({ userId: m.user_id, role: e.target.value as Member["role"] })
                      }
                      className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
                    >
                      {ROLES.map((r) => (
                        <option key={r} value={r}>{r}</option>
                      ))}
                    </select>
                    <button
                      type="button"
                      aria-label={`remove @${m.handle}`}
                      onClick={() => {
                        if (confirm(`Remove @${m.handle} from this project? You can add them back anytime.`))
                          remove.mutate(m.user_id);
                      }}
                      className="text-chrome-dim hover:text-red-300"
                    >
                      <X size={14} />
                    </button>
                  </>
                ) : (
                  <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-widest text-chrome-dim">
                    {m.role}
                  </span>
                )}
              </div>
            </li>
          ))}
          {members.length === 0 && !membersQ.isLoading && (
            <li className="mono text-[11px] text-chrome-dim">no members yet</li>
          )}
          {membersQ.isLoading && (
            <li className="mono text-[11px] text-chrome-dim">git fetch --rebase your-stuff…</li>
          )}
        </ul>

        {canManage && (
          <div className="space-y-2 border-t border-white/10 pt-3">
            <div className="mono flex items-center gap-2 text-[11px] text-chrome-dim">
              <UserPlus size={12} /> add someone
              <select
                aria-label="role for the new member"
                value={addRole}
                onChange={(e) => setAddRole(e.target.value as Member["role"])}
                className="ml-auto rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
              >
                {ROLES.map((r) => (
                  <option key={r} value={r}>{r}</option>
                ))}
              </select>
            </div>
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="find a user by handle or email…"
              aria-label="find a user to add"
              className="mono w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none placeholder:text-chrome-dim/50 placeholder:italic"
            />
            {q.trim().length >= 2 && (
              <ul className="max-h-48 space-y-0.5 overflow-y-auto">
                {candidates.map((u) => (
                  <li key={u.id}>
                    <button
                      type="button"
                      disabled={add.isPending}
                      onClick={() => add.mutate(u.id)}
                      className="mono flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-white/5 disabled:opacity-50"
                    >
                      <span className="text-chrome">@{u.handle}</span>
                      <span className="truncate text-chrome-dim">{u.display_name}</span>
                      <UserPlus size={11} className="ml-auto text-accent" />
                    </button>
                  </li>
                ))}
                {!hitsQ.isFetching && candidates.length === 0 && (
                  <li className="mono px-2 py-1 text-[11px] text-chrome-dim">
                    {hitsQ.data && (hitsQ.data.users?.length ?? 0) > 0
                      ? "already on the project"
                      : "nobody matches — check the handle or email"}
                  </li>
                )}
              </ul>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
