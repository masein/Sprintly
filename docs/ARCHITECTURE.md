# Architecture

> Snapshot at the end of M1 phase 1. Updated as features land.

## Shape

```
                           ┌──────────────┐
   browser  ─── HTTPS ───▶│    Caddy     │ :443 (prod) / :8080 (dev)
                           └──────┬───────┘
                                  │
                  ┌───────────────┴───────────────┐
                  │                               │
            ┌─────▼─────┐                   ┌─────▼─────┐
            │  Next.js  │                   │   Axum    │
            │  (web)    │ ── REST / WS ────▶│   (api)   │
            └───────────┘                   └─────┬─────┘
                                                  │
                  ┌──────────────┬────────────────┼────────────────┐
                  │              │                │                │
            ┌─────▼─────┐  ┌─────▼─────┐    ┌─────▼─────┐    ┌─────▼─────┐
            │ Postgres  │  │   Redis   │    │   MinIO   │    │  pg_dump  │
            │    16     │  │     7     │    │ (S3 API)  │    │ (backups) │
            └───────────┘  └───────────┘    └───────────┘    └───────────┘
```

Every box is a docker-compose service. Postgres / Redis / MinIO never face the
internet — Caddy is the only public surface.

## Why these choices

- **Axum + SQLx, not a heavier ORM.** Compile-time-checked SQL keeps us honest
  about query shape and indexes. No N+1 surprises hiding inside lazy loaders.
- **Single binary for API + workers.** `sprintly-api`, `sprintly-api migrate`,
  and `sprintly-api healthcheck` are subcommands of the same image. One artifact,
  one set of env vars.
- **Redis for fan-out.** When we run multiple API replicas in M-something, the
  WebSocket layer pub/subs through Redis so any replica can push to any client.
- **MinIO over local filesystem.** Attachments must survive container restarts
  and be backup-able without snapshotting the API container's disk.
- **No GraphQL.** REST plus a small filter DSL is enough for what we're doing
  and pairs cleanly with cursor pagination.

## Boot flow

1. `postgres` and `redis` start, healthchecks settle.
2. `minio-init` creates the bucket if it doesn't exist, then exits.
3. `migrate` runs SQLx migrations against Postgres, then exits 0.
4. `api` boots after `migrate` exits successfully — guaranteed schema at start.
5. `web` boots after `api` is healthy.
6. `caddy` fronts both. `/api/*` and `/ws` → api; `/*` → web.

## Process model

The Rust binary is one Tokio runtime. Background workers will share the
runtime, with a `jobs` table providing durability (added when there's a job
worth running — likely M3, when activity feed indexing arrives).

## What lives where (M1 phase 1)

| Concern                | Module / path                                  |
| ---------------------- | ---------------------------------------------- |
| Entry / subcommands    | `apps/api/src/main.rs`                         |
| Router composition     | `apps/api/src/app.rs`                          |
| Env config             | `apps/api/src/config.rs`                       |
| Errors                 | `apps/api/src/error.rs`                        |
| DB / Redis clients     | `apps/api/src/infra/`                          |
| HTTP handlers          | `apps/api/src/routes/`                         |
| Pure logic (auth etc.) | `apps/api/src/domain/`                         |
| Frontend pages         | `apps/web/app/`                                |
| Frontend tokens        | `apps/web/tailwind.config.ts`                  |
| Reverse proxy          | `infra/docker/caddy/Caddyfile`                 |
| Compose                | `infra/compose/docker-compose*.yml`            |

## Realtime (M3-A)

```
HTTP write handler ── publish(Event) ──▶ Redis PUBLISH sprintly:events
                                                       │
            ┌─────────────────────────┬────────────────┼────────────────┐
            │                         │                │                │
       /ws conn A                /ws conn B       /ws conn C       … (any replica)
            │                         │                │
       filter on accessible projects (per-user)
            │
       JSON frame → browser → TanStack Query invalidate
```

- **One channel: `sprintly:events`.** Every event carries `project_id`; the
  WS handler filters per connection. Trade-off discussed in
  `apps/api/src/infra/events.rs`.
- **Dedicated Redis connection per WS session.** `deadpool-redis` is for
  pooled non-pubsub work; pubsub parks a connection so we open a fresh one.
- **Optimistic UI.** TanStack Query mutations apply the move locally first,
  then settle on the server response. WS events trigger query invalidations
  so a second browser tab catches up without polling.
- **Heartbeat ping every 20s.** Membership snapshot refresh every 30s.

## Open decisions (deferred)

- **Static asset CDN.** Punted to prod-tuning later; standalone Next handles it.
- **Email delivery.** Out of scope for v1 (per spec §1 non-goals). Password
  reset tokens get rendered in the UI for now.
- **Multi-instance API.** Designed for it (stateless API, Redis pub/sub) but
  v1 ships single-instance.
