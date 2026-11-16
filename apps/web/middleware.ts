// Per-request Content-Security-Policy with a nonce (see lib/csp.ts).
//
// The nonce goes on the *request* headers too: Next.js reads the request's
// Content-Security-Policy to find it and adds it to every script it renders.
// That only works for pages rendered per request, which is why the root
// layout forces dynamic rendering.

import { NextResponse, type NextRequest } from "next/server";
import { buildCsp, generateNonce } from "@/lib/csp";

export function middleware(request: NextRequest) {
  const nonce = generateNonce();
  const csp = buildCsp(nonce, { dev: process.env.NODE_ENV === "development" });

  const requestHeaders = new Headers(request.headers);
  requestHeaders.set("x-nonce", nonce);
  requestHeaders.set("Content-Security-Policy", csp);

  const response = NextResponse.next({ request: { headers: requestHeaders } });
  response.headers.set("Content-Security-Policy", csp);
  return response;
}

export const config = {
  matcher: [
    {
      // Pages only — not the API route, static assets, or the service worker
      // and icons (no document, no scripts to protect).
      source:
        "/((?!api/|_next/static|_next/image|sw\\.js|icon\\.svg|manifest\\.webmanifest|favicon\\.ico).*)",
      // Prefetches are rendered later as part of a real navigation.
      missing: [
        { type: "header", key: "next-router-prefetch" },
        { type: "header", key: "purpose", value: "prefetch" },
      ],
    },
  ],
};
