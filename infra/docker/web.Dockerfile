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
FROM base AS builder
ENV NEXT_TELEMETRY_DISABLED=1
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
# `next build` with output:"standalone" (no outputFileTracingRoot) roots the
# traced bundle at the app, so server.js sits at the standalone root and the
# server serves ./.next/static and ./public relative to it.
COPY --from=builder --chown=sprintly:sprintly /app/apps/web/.next/standalone ./
COPY --from=builder --chown=sprintly:sprintly /app/apps/web/.next/static ./.next/static
COPY --from=builder --chown=sprintly:sprintly /app/apps/web/public ./public
USER sprintly
EXPOSE 3000
CMD ["node", "server.js"]
