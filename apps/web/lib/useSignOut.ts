"use client";

// The one way to sign out. Every logout entry point (session badge, its
// mobile menu, the command palette) must do the same full teardown — the
// palette's old version only called the API and navigated, leaving the
// previous user's query cache and realtime socket alive for whoever signed in
// next in the same tab.

import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { logout } from "@/lib/auth-bundle";
import { disableRealtime } from "@/lib/ws";
import { markSignedOut } from "@/lib/session-signal";

export function useSignOut(): () => Promise<void> {
  const qc = useQueryClient();
  const router = useRouter();
  return useCallback(async () => {
    // Revoke the session server-side; a network error mustn't keep anyone
    // half-signed-in on this device, so carry on regardless.
    await logout().catch(() => {});
    disableRealtime();
    markSignedOut();
    // Nothing from this session may be served to the next one: cached
    // queries (tasks, people), and pages the service worker kept for
    // offline use (public/sw.js drops its page cache on this message).
    qc.clear();
    navigator.serviceWorker?.controller?.postMessage({ type: "sprintly:signed-out" });
    router.push("/login");
  }, [qc, router]);
}
