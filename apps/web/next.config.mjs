/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  reactStrictMode: true,
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
