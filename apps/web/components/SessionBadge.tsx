"use client";

// Tiny client component the landing page uses to show who's signed in.
// Calls /users/me; on 401 the api wrapper attempts a refresh exactly once.

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { me, logout, type Me } from "@/lib/auth-bundle";

export function SessionBadge() {
  const router = useRouter();
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
          router.refresh();
          setUser(null);
        }}
        className="text-accent hover:underline"
      >
        logout
      </button>
    </div>
  );
}
