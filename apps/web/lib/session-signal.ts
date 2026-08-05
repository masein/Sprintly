// A one-bit "is somebody signed in?" signal, shared across the tab.
//
// The session itself lives in httpOnly cookies, so JavaScript can't read it —
// the only way to know is to ask the API. Components that poll authed
// endpoints (achievements, and anything similar later) used to just fire and
// swallow the 401, which meant every visit to /login or the landing page threw
// console errors for requests that could never succeed. This lets those
// components wait for a session instead of guessing.
//
// Providers sets it from its own `me()` probe on load; AuthForm sets it the
// moment a sign-in succeeds (so nothing waits for a reload), and SessionBadge
// clears it on logout.

type Watcher = (signedIn: boolean) => void;

let signedIn = false;
const watchers = new Set<Watcher>();

export function isSignedIn(): boolean {
  return signedIn;
}

export function markSignedIn(): void {
  if (signedIn) return;
  signedIn = true;
  for (const fn of watchers) fn(true);
}

export function markSignedOut(): void {
  if (!signedIn) return;
  signedIn = false;
  for (const fn of watchers) fn(false);
}

/** Subscribe to changes. Returns an unsubscribe function. */
export function onSessionChange(fn: Watcher): () => void {
  watchers.add(fn);
  return () => watchers.delete(fn);
}
