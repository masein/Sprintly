"use client";

// Signed-in visitors to "/" get sent to My Day — that's where a working day
// starts, and the landing copy is for people who aren't signed in yet.
// Deliberately client-side: the session is an HttpOnly cookie the API
// validates, so "am I signed in?" is one /users/me call, not a server read.
// `replace` keeps the landing page out of the back-button history, so
// "back" from My Day doesn't bounce you forward again.

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { me } from "@/lib/auth-bundle";

export function LandingRedirect() {
  const router = useRouter();
  useEffect(() => {
    let cancelled = false;
    void me()
      .then(() => {
        if (!cancelled) router.replace("/me/day");
      })
      // Not signed in (401) — stay here and read the pitch.
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [router]);
  return null;
}
