"use client";

// Tiny client component the landing page uses to show who's signed in.
// Calls /users/me; on 401 the api wrapper attempts a refresh exactly once.

import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { me, logout, type Me } from "@/lib/auth-bundle";
import { Avatar } from "./Avatar";

export function SessionBadge() {
  const router = useRouter();
  const qc = useQueryClient();
  const [user, setUser] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    me()
      .then((u) => {
        if (alive) setUser(u);
      })
      .catch(() => {
        if (alive) setUser(null);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  if (loading) {
    return (
      <div className="mono text-xs text-chrome-dim">
        git fetch --rebase your-stuff…
      </div>
    );
  }

  if (!user) {
    return (
      <div className="mono flex items-center gap-3 text-xs">
        <Link href="/login" className="text-accent hover:underline">
          sign in
        </Link>
        <span className="text-chrome-dim">·</span>
        <Link href="/register" className="text-accent hover:underline">
          register
        </Link>
      </div>
    );
  }

  return (
    <div className="mono flex items-center gap-3 text-xs">
      <span className="text-chrome-dim">signed in as</span>
      <Avatar
        size={20}
        user={{
          userId: user.id,
          displayName: user.display_name,
          handle: user.handle,
          avatarUrl: user.avatar_url,
          avatarStyle: user.avatar_style,
          avatarSeed: user.avatar_seed,
        }}
      />
      <span className="text-chrome">@{user.handle}</span>
      <span className="rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-widest text-chrome-dim">
        {user.role}
      </span>
      <span className="text-chrome-dim">·</span>
      <Link href="/me/day" className="text-accent hover:underline">
        my day
      </Link>
      <span className="text-chrome-dim">·</span>
      <Link href="/settings" className="text-accent hover:underline">
        settings
      </Link>
      <span className="text-chrome-dim">·</span>
      <button
        type="button"
        onClick={async () => {
          await logout().catch(() => {});
          setUser(null);
          // Land on the sign-in page with nothing cached — staying put with
          // stale query data made logout look like it did nothing.
          qc.clear();
          router.push("/login");
        }}
        className="text-accent hover:underline"
      >
        logout
      </button>
    </div>
  );
}

// Stacked, menu-friendly rendering of the same session actions — used in the
// header's mobile dropdown, where there's no room for the inline SessionBadge
// row (that starvation was the whole header-overflow bug).
export function SessionMenuContents({ onNavigate }: { onNavigate?: () => void }) {
  const router = useRouter();
  const qc = useQueryClient();
  const [user, setUser] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    me()
      .then((u) => {
        if (alive) setUser(u);
      })
      .catch(() => {
        if (alive) setUser(null);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  if (loading) {
    return (
      <div className="mono px-2 py-2 text-xs text-chrome-dim">
        git fetch --rebase your-stuff…
      </div>
    );
  }

  if (!user) {
    return (
      <div className="mono flex flex-col gap-1 p-1 text-xs">
        <Link
          href="/login"
          onClick={onNavigate}
          className="rounded px-2 py-1.5 text-accent hover:bg-white/5"
        >
          sign in
        </Link>
        <Link
          href="/register"
          onClick={onNavigate}
          className="rounded px-2 py-1.5 text-accent hover:bg-white/5"
        >
          register
        </Link>
      </div>
    );
  }

  return (
    <div className="mono flex flex-col text-xs">
      <div className="flex items-center gap-2 px-2 py-1.5">
        <Avatar
          size={20}
          user={{
            userId: user.id,
            displayName: user.display_name,
            handle: user.handle,
            avatarUrl: user.avatar_url,
            avatarStyle: user.avatar_style,
            avatarSeed: user.avatar_seed,
          }}
        />
        <span className="truncate text-chrome">@{user.handle}</span>
        <span className="ml-auto shrink-0 rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-widest text-chrome-dim">
          {user.role}
        </span>
      </div>
      <div className="my-1 border-t border-white/10" />
      <Link
        href="/me/day"
        onClick={onNavigate}
        className="rounded px-2 py-1.5 text-left hover:bg-white/5"
      >
        my day
      </Link>
      <Link
        href="/settings"
        onClick={onNavigate}
        className="rounded px-2 py-1.5 text-left hover:bg-white/5"
      >
        settings
      </Link>
      <button
        type="button"
        onClick={async () => {
          onNavigate?.();
          await logout().catch(() => {});
          setUser(null);
          qc.clear();
          router.push("/login");
        }}
        className="rounded px-2 py-1.5 text-left hover:bg-white/5"
      >
        logout
      </button>
    </div>
  );
}
