// Sprintly service worker (F17). App-shell + read cache with an offline
// fallback. Deliberately conservative: never caches the API (auth-bearing,
// must stay fresh), storage (/s3 — presigned, per-user), or cross-origin
// requests.
//
// Two caches:
//   SHELL — the offline fallback and content-hashed static assets. Safe to
//           keep across sessions; nothing user-specific lives here.
//   PAGES — page navigations, for offline reloads. Dropped on sign-out
//           (see the "sprintly:signed-out" message below) so a shared
//           machine doesn't keep the previous user's pages.
// v2 because v1 cached every navigation, including /reset?token=… and
// /register?invite=… URLs — activating v2 deletes those caches.

const SHELL_CACHE = "sprintly-shell-v2";
const PAGES_CACHE = "sprintly-pages-v2";
const SHELL = ["/", "/offline", "/icon.svg", "/manifest.webmanifest"];

// Pages that carry (or are) secrets in their URL — never cached.
const NO_CACHE_PREFIXES = ["/reset", "/register", "/status/", "/login"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(SHELL_CACHE)
      .then((c) => c.addAll(SHELL))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  const keep = new Set([SHELL_CACHE, PAGES_CACHE]);
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => !keep.has(k)).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

// The app posts this on sign-out (lib/useSignOut): forget cached pages.
self.addEventListener("message", (event) => {
  if (event.data && event.data.type === "sprintly:signed-out") {
    event.waitUntil(caches.delete(PAGES_CACHE));
  }
});

/** A navigation we may keep: a plain path (no query — that's where tokens
 * ride), not one of the token-bearing pages. */
function cacheablePage(url) {
  if (url.search !== "") return false;
  return !NO_CACHE_PREFIXES.some((p) => url.pathname === p || url.pathname.startsWith(p));
}

/** Static and content-hashed: safe to serve cache-first indefinitely. */
function cacheableAsset(url) {
  if (url.search !== "") return false; // incl. Next's ?_rsc= payloads
  return (
    url.pathname.startsWith("/_next/static/") ||
    SHELL.includes(url.pathname) ||
    /\.(?:svg|png|ico|webp|woff2?|webmanifest)$/.test(url.pathname)
  );
}

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") return;

  const url = new URL(req.url);
  // Same-origin only, and never the API or storage.
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/s3/")) return;

  // Page navigations: network-first → cache → offline page.
  if (req.mode === "navigate") {
    event.respondWith(
      fetch(req)
        .then((res) => {
          // Only successful same-origin pages; never error pages, redirects
          // or token-bearing URLs.
          if (res.ok && res.type === "basic" && cacheablePage(url)) {
            const copy = res.clone();
            caches.open(PAGES_CACHE).then((c) => c.put(req, copy));
          }
          return res;
        })
        .catch(() => caches.match(req).then((c) => c || caches.match("/offline"))),
    );
    return;
  }

  // Static assets: cache-first, fill on miss. Anything else goes straight
  // to the network, uncached.
  if (!cacheableAsset(url)) return;
  event.respondWith(
    caches.match(req).then(
      (cached) =>
        cached ||
        fetch(req).then((res) => {
          if (res.ok && res.type === "basic") {
            const copy = res.clone();
            caches.open(SHELL_CACHE).then((c) => c.put(req, copy));
          }
          return res;
        }),
    ),
  );
});
