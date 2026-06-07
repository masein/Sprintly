"use client";

// Header notification bell. Unread badge + dropdown inbox. Live-updates via the
// WS layer, which invalidates the ["notifications"] query on notification_created.

import { useEffect, useRef, useState } from "react";
import type { Route } from "next";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AtSign, Bell, CheckCheck, MessageSquare, UserPlus } from "lucide-react";
import {
  listNotifications,
  markAllRead,
  markRead,
  unreadCount,
  type Notification,
} from "@/lib/notifications";

const KIND_ICON: Record<Notification["kind"], React.ComponentType<{ size?: string | number }>> = {
  mention: AtSign,
  assigned: UserPlus,
  comment: MessageSquare,
};

export function NotificationBell() {
  const qc = useQueryClient();
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const count = useQuery({
    queryKey: ["notifications", "unread-count"],
    queryFn: unreadCount,
    retry: false,
  });
  const list = useQuery({
    queryKey: ["notifications", "list"],
    queryFn: listNotifications,
    enabled: open,
    retry: false,
  });

  const read = useMutation({
    mutationFn: (id: string) => markRead(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["notifications"] }),
  });
  const readAll = useMutation({
    mutationFn: () => markAllRead(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["notifications"] }),
  });

  // Close on outside click / Escape.
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", onClick);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const unread = count.data?.count ?? 0;

  function activate(n: Notification) {
    if (!n.read_at) read.mutate(n.id);
    setOpen(false);
    // Links come from the API as plain strings; typedRoutes needs a cast.
    if (n.link) router.push(n.link as Route);
  }

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-label="Notifications"
        onClick={() => setOpen((o) => !o)}
        className="relative grid h-8 w-8 place-items-center rounded text-chrome-dim hover:bg-white/5 hover:text-chrome"
      >
        <Bell size={16} />
        {unread > 0 && (
          <span className="mono absolute -right-0.5 -top-0.5 grid h-4 min-w-4 place-items-center rounded-full bg-accent px-1 text-[10px] font-medium text-accent-fg">
            {unread > 9 ? "9+" : unread}
          </span>
        )}
      </button>

      {open && (
        <div
          role="menu"
          className="absolute right-0 z-30 mt-2 w-80 overflow-hidden rounded-lg border border-white/10 bg-ink-subtle shadow-xl"
        >
          <div className="flex items-center justify-between border-b border-white/10 px-3 py-2">
            <span className="mono text-xs uppercase tracking-widest text-chrome-dim">
              notifications
            </span>
            {unread > 0 && (
              <button
                type="button"
                onClick={() => readAll.mutate()}
                disabled={readAll.isPending}
                className="mono inline-flex items-center gap-1 text-[11px] text-chrome-dim hover:text-chrome disabled:opacity-50"
              >
                <CheckCheck size={12} /> mark all read
              </button>
            )}
          </div>

          <div className="max-h-96 overflow-y-auto">
            {list.isLoading ? (
              <div className="mono px-3 py-6 text-center text-xs text-chrome-dim">
                nudging electrons…
              </div>
            ) : (list.data?.length ?? 0) === 0 ? (
              <div className="mono px-3 py-6 text-center text-xs text-chrome-dim">
                Inbox zero. Touch grass.
              </div>
            ) : (
              <ul>
                {list.data!.map((n) => {
                  const Icon = KIND_ICON[n.kind] ?? Bell;
                  return (
                    <li key={n.id}>
                      <button
                        type="button"
                        onClick={() => activate(n)}
                        className={`flex w-full items-start gap-2 border-b border-white/5 px-3 py-2 text-left hover:bg-white/5 ${
                          n.read_at ? "opacity-60" : ""
                        }`}
                      >
                        <Icon size={14} />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-xs text-chrome">
                            {n.title}
                          </span>
                          {n.body && (
                            <span className="mono block truncate text-[11px] text-chrome-dim">
                              {n.actor_handle ? `@${n.actor_handle}: ` : ""}
                              {n.body}
                            </span>
                          )}
                        </span>
                        {!n.read_at && (
                          <span className="mt-1 h-1.5 w-1.5 flex-shrink-0 rounded-full bg-accent" />
                        )}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
