// Offline fallback (F17). The service worker serves this for navigations when
// the network is gone. Standalone — no AppShell (which would try to fetch).

export const metadata = { title: "Offline · Sprintly" };

export default function OfflinePage() {
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-4 px-6 py-20 text-center">
      <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
        sprintly · offline
      </div>
      <h1 className="text-2xl font-semibold">No connection.</h1>
      <p className="text-sm text-chrome-dim">
        You&apos;re offline, so this is as far as we can go. The app shell is
        cached — reconnect and it&apos;ll pick up where you left off.
      </p>
      <p className="mono text-[11px] text-chrome-dim">
        $ ping -c1 the-internet
      </p>
    </main>
  );
}
