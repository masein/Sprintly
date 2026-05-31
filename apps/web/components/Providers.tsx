"use client";

// Root client providers: TanStack QueryClient + WS connector + theme + hotkeys.
// Wraps the app inside <body>. Server components above stay server.

import { useEffect, useState } from "react";
import {
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { connectWs } from "@/lib/ws";
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

  // Open one shared WS connection per tab; close it on unmount.
  useEffect(() => connectWs(client), [client]);

  return (
    <QueryClientProvider client={client}>
      {children}
      <KeyboardHotkeys />
      <AchievementToast />
    </QueryClientProvider>
  );
}
