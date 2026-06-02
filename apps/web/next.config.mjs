import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  reactStrictMode: true,
  // Pin the standalone tracing root to the monorepo root so the output layout
  // is deterministic (apps/web/server.js + hoisted node_modules) regardless of
  // whether a workspace lockfile is present. The Dockerfile relies on this.
  outputFileTracingRoot: path.join(__dirname, "../../"),
  // Anything heavier (image domains, redirects, headers) lands when we need it.
  experimental: {
    typedRoutes: true,
  },
  async rewrites() {
    // In dev (running `pnpm dev` outside the Caddy stack), proxy API calls to
    // the API container directly. In prod and dev-via-compose, Caddy handles
    // this and these rewrites never fire because the paths are the same host.
    return [];
  },
};

export default nextConfig;
