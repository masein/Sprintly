"use client";

// Root client providers: TanStack QueryClient + WS connector + theme + hotkeys.
// Wraps the app inside <body>. Server components above stay server.

import { useEffect, useState } from "react";
import {
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { enableRealtime } from "@/lib/ws";
import { markSignedIn } from "@/lib/session-signal";
import { me } from "@/lib/auth-bundle";
import { applyStoredTheme } from "@/lib/theme";
import { KeyboardHotkeys } from "./KeyboardHotkeys";
import { AchievementToast } from "./AchievementToast";

export function Providers({ children }: { children: React.ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 30_000,
            retry: 1,
            refetchOnWindowFocus: false,
          },
        },
      }),
  );

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
    </QueryClientProvider>
  );
}
