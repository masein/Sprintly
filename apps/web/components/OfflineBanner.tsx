"use client";

// A thin banner shown while the browser reports no connection (F17). Honest,
// not alarmist — actions just won't reach the server until you're back.

import { useEffect, useState } from "react";
import { WifiOff } from "lucide-react";

export function OfflineBanner() {
  const [offline, setOffline] = useState(false);

  useEffect(() => {
    const sync = () => setOffline(!navigator.onLine);
    sync();
    window.addEventListener("online", sync);
    window.addEventListener("offline", sync);
    return () => {
      window.removeEventListener("online", sync);
      window.removeEventListener("offline", sync);
    };
  }, []);

  if (!offline) return null;
  return (
    <div
      role="status"
      className="mono flex items-center justify-center gap-2 bg-amber-500/20 px-4 py-1.5 text-[11px] text-amber-200"
    >
      <WifiOff size={12} /> you&apos;re offline — changes won&apos;t save until you reconnect
    </div>
  );
}
