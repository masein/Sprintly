"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, EyeOff } from "lucide-react";
import { addWatcher, listWatchers, removeWatcher } from "@/lib/task-detail";
import { me } from "@/lib/auth-bundle";
import { Avatar } from "./Avatar";

export function Watchers({ taskKey }: { taskKey: string }) {
  const qc = useQueryClient();
  const w = useQuery({ queryKey: ["watchers", taskKey], queryFn: () => listWatchers(taskKey) });
  const user = useQuery({ queryKey: ["me"], queryFn: () => me() });

  const isWatching = !!user.data && (w.data ?? []).some((x) => x.user_id === user.data!.id);

  const add = useMutation({
    mutationFn: () => addWatcher(taskKey, user.data!.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["watchers", taskKey] }),
  });
  const remove = useMutation({
    mutationFn: () => removeWatcher(taskKey, user.data!.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["watchers", taskKey] }),
  });

  return (
    <section className="space-y-2">
      <h2 className="mono flex items-center justify-between text-xs uppercase tracking-widest text-chrome-dim">
        <span>watchers ({w.data?.length ?? 0})</span>
        {user.data && (
          <button
            type="button"
            onClick={() => (isWatching ? remove.mutate() : add.mutate())}
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
        {(w.data ?? []).map((w) => (
          <li key={w.user_id} className="mono flex items-center gap-2 text-xs">
            <Avatar
              size={18}
              user={{
                userId: w.user_id,
                displayName: w.display_name,
                handle: w.handle,
                avatarUrl: w.avatar_url,
                avatarStyle: w.avatar_style,
                avatarSeed: w.avatar_seed,
              }}
            />
            <span className="text-chrome">@{w.handle}</span>
            <span className="text-chrome-dim">{w.display_name}</span>
          </li>
        ))}
        {w.data?.length === 0 && (
          <li className="mono text-[11px] text-chrome-dim">no watchers</li>
        )}
      </ul>
    </section>
  );
}
