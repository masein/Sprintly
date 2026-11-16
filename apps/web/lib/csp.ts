// The Content-Security-Policy for the app's pages, with a per-request nonce.
//
// middleware.ts mints a fresh nonce for every page request and sends this
// policy; Next.js reads the nonce back from the request's CSP header and
// stamps it on every script it emits (framework chunks and the inline RSC
// payloads). Scripts without the nonce — anything injected into the page —
// don't run. Before this, script-src had to allow 'unsafe-inline' for
// Next's bootstrap scripts, which made the CSP no backstop against XSS.
//
// script-src, as browsers read it:
//   CSP3 (all current browsers): 'nonce-…' + 'strict-dynamic' — only nonced
//     scripts, plus what they load (webpack chunks). 'self', https: and
//     'unsafe-inline' are ignored once a nonce / strict-dynamic is present.
//   Older browsers fall back to those, so the app still loads there.
// (Google's "strict CSP" recommendation, https://csp.withgoogle.com.)
//
// Styles keep 'unsafe-inline': React style={{…}} attributes can't carry a
// nonce, and style injection is far less dangerous than script.

/** 16 random bytes, base64 — a fresh one per request. */
export function generateNonce(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes));
}

export function buildCsp(nonce: string, opts: { dev?: boolean } = {}): string {
  const directives: Record<string, string[]> = {
    "default-src": ["'self'"],
    "script-src": [
      `'nonce-${nonce}'`,
      "'strict-dynamic'",
      // Fallbacks for pre-CSP3 browsers only (see the header comment).
      "'self'",
      "https:",
      "'unsafe-inline'",
      // React Refresh / webpack eval in `next dev` only — never production.
      ...(opts.dev ? ["'unsafe-eval'"] : []),
    ],
    "style-src": ["'self'", "'unsafe-inline'", "https://rsms.me"],
    "font-src": ["'self'", "data:", "https://rsms.me"],
    // Avatars and markdown images may be any https URL.
    "img-src": ["'self'", "data:", "blob:", "https:"],
    // Realtime socket (older Safari doesn't count ws: as 'self').
    "connect-src": ["'self'", "ws:", "wss:"],
    "worker-src": ["'self'"],
    "manifest-src": ["'self'"],
    "frame-src": ["'none'"],
    "frame-ancestors": ["'none'"],
    "object-src": ["'none'"],
    "base-uri": ["'self'"],
    "form-action": ["'self'"],
  };
  return Object.entries(directives)
    .map(([k, v]) => `${k} ${v.join(" ")}`)
    .join("; ");
}
