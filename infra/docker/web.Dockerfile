# syntax=docker/dockerfile:1.7

# ─────────────────────────────────────────────────────────────────────────────
# Sprintly Web — multi-stage Dockerfile.
#
# Stages:
#   base     — Node 20 + pnpm
#   deps     — install workspace dependencies (cached on lockfiles)
#   dev      — hot-reload dev with `pnpm dev`
#   builder  — `next build` for prod
#   runtime  — minimal prod image with standalone Next output
# ─────────────────────────────────────────────────────────────────────────────

FROM node:20-alpine AS base
RUN corepack enable && corepack prepare pnpm@9.12.0 --activate
WORKDIR /app

# ─── Deps: install once, cached on lockfile + manifests ──────────────────
FROM base AS deps
# All workspace manifests + the committed lockfile so --frozen-lockfile can
# validate; --filter installs only web and its workspace deps (not e2e/playwright).
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/web/package.json apps/web/
COPY apps/e2e/package.json apps/e2e/
COPY packages ./packages
RUN pnpm install --frozen-lockfile --filter "@sprintly/web..."

# ─── Dev image ───────────────────────────────────────────────────────────
FROM base AS dev
ENV NEXT_TELEMETRY_DISABLED=1
COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /app/apps/web/node_modules ./apps/web/node_modules
COPY . .
WORKDIR /app/apps/web
EXPOSE 3000
CMD ["pnpm", "dev"]

# ─── Builder: produce a standalone Next build ────────────────────────────
# NEXT_PUBLIC_* are inlined into the client bundle at build time, so they must
# be set here — not at container runtime. Defaults match the source fallbacks
# so a plain `just up` build is unchanged. The prod/registry build overrides
# them to same-origin values (API_BASE_URL=/api/v1, WS_URL=/ws) via --build-arg
# so one image works behind any host/scheme without a rebuild (see release.yml).
FROM base AS builder
ENV NEXT_TELEMETRY_DISABLED=1
ARG NEXT_PUBLIC_API_BASE_URL=http://localhost:8080/api/v1
ARG NEXT_PUBLIC_WS_URL=ws://localhost:8080/ws
ARG NEXT_PUBLIC_APP_NAME=Sprintly
ENV NEXT_PUBLIC_API_BASE_URL=$NEXT_PUBLIC_API_BASE_URL \
    NEXT_PUBLIC_WS_URL=$NEXT_PUBLIC_WS_URL \
    NEXT_PUBLIC_APP_NAME=$NEXT_PUBLIC_APP_NAME
COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /app/apps/web/node_modules ./apps/web/node_modules
COPY . .
WORKDIR /app/apps/web
RUN pnpm build

# ─── Runtime ─────────────────────────────────────────────────────────────
FROM node:20-alpine AS runtime
ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
# Next's standalone server binds to $HOSTNAME, which Docker otherwise sets to
# the container id — that leaves localhost unbound and fails the healthcheck.
ENV HOSTNAME=0.0.0.0
RUN apk add --no-cache wget && \
    addgroup -S sprintly && adduser -S sprintly -G sprintly
WORKDIR /app
# With outputFileTracingRoot pinned to the monorepo root, the standalone bundle
# mirrors the repo: apps/web/server.js + hoisted node_modules at the root. Copy
# the traced static/public next to the server.
COPY --from=builder --chown=sprintly:sprintly /app/apps/web/.next/standalone ./
COPY --from=builder --chown=sprintly:sprintly /app/apps/web/.next/static ./apps/web/.next/static
COPY --from=builder --chown=sprintly:sprintly /app/apps/web/public ./apps/web/public
USER sprintly
EXPOSE 3000
CMD ["node", "apps/web/server.js"]
