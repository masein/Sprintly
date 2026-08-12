"use client";

// Root client providers: TanStack QueryClient + WS connector + theme + hotkeys.
// Wraps the app inside <body>. Server components above stay server.

import { useEffect, useState } from "react";
import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { enableRealtime } from "@/lib/ws";
import { markSignedIn } from "@/lib/session-signal";
import { me } from "@/lib/auth-bundle";
import { applyStoredTheme } from "@/lib/theme";
import { KeyboardHotkeys } from "./KeyboardHotkeys";
import { AchievementToast } from "./AchievementToast";
import { Toaster } from "./Toaster";

export function Providers({ children }: { children: React.ReactNode }) {
  const [client] = useState(() => {
    // Every successful mutation invalidates everything. Sounds heavy; isn't:
    // only mounted queries actually refetch, and staleTime keeps the rest
    // quiet. This is the app-wide answer to "I changed X and the screen
    // didn't notice" — no call site needs to remember its own invalidations.
    const qc: QueryClient = new QueryClient({
      mutationCache: new MutationCache({
        onSuccess: () => qc.invalidateQueries(),
      }),
      defaultOptions: {
        queries: {
          staleTime: 30_000,
          retry: 1,
          // Coming back to a dulled tab is exactly when stale data shows.
          refetchOnWindowFocus: true,
          refetchOnReconnect: true,
        },
      },
    });
    return qc;
  });

  // Apply the saved theme as early as the client can — before paint where
  // possible. The flash is brief because Next pre-renders with the default.
  useEffect(() => applyStoredTheme(), []);

  // Open one shared WS connection per tab — but only for a signed-in
  // session. Connecting unconditionally meant /login and the landing page
  // fired failed handshakes (console errors, and back/forward-cache blocked)
  // for visitors who have no session to subscribe with. AuthForm calls
  // enableRealtime() itself the moment a sign-in succeeds, so realtime is
  // live without waiting for a reload.
  useEffect(() => {
    let cancelled = false;
    void me()
      .then(() => {
        if (cancelled) return;
        markSignedIn();
        enableRealtime(client);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [client]);

  return (
    <QueryClientProvider client={client}>
      {children}
      <KeyboardHotkeys />
      <AchievementToast />
      <Toaster />
    </QueryClientProvider>
  );
}
