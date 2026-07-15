# Sprintly — Production Runbook (air-gapped host + private registry)

This is the operational runbook for the deployment model where:

- The **target server** can reach only **you (SSH)** and the **private registry**
  `REGISTRY_HOST` (`docker.netixsystem.com`). It has **no public internet**.
  Every image it runs must already live in that registry.
- Only the **web/frontend port** (Caddy, HTTP) is published on the host. `api`,
  `web`, `postgres`, `redis`, and `minio` stay on the internal compose network.
- Secrets live in an on-server `.env` (fail-fast; nothing is committed).
- The API applies DB migrations itself at startup — no separate migrate step.

Files that make this work (all in this repo):

| File | Role |
|------|------|
| [`infra/compose/docker-compose.prod.yml`](../infra/compose/docker-compose.prod.yml) | Self-contained prod stack; every `image:` is `$REGISTRY_HOST/...` |
| [`infra/docker/caddy/Caddyfile.prod`](../infra/docker/caddy/Caddyfile.prod) | HTTP-only reverse proxy (no public ACME on an air-gapped box) |
| [`.env.prod.example`](../.env.prod.example) | Env template — copy to `.env` on the server, fill in |
| [`.github/workflows/release.yml`](../.github/workflows/release.yml) | On merge to `main`: build + push app images to the registry |

> **Roles.** Three machines appear below:
> - **CI / GitHub Actions** — builds and pushes the *app* images (`sprintly-api`, `sprintly-web`).
> - **Dual-access machine** — a workstation that can reach *both* Docker Hub and `REGISTRY_HOST`. Used once to mirror *third-party* base images (Postgres, Redis, MinIO, mc, Caddy) into the private registry. It can also stand in for CI to push app images if the registry isn't reachable from GitHub runners.
> - **Server** — the air-gapped prod host. Only pulls from `REGISTRY_HOST`; never builds.

---

## 0. Conventions

Set these in your shell where noted (examples):

```sh
export REGISTRY_HOST=docker.netixsystem.com
export SERVER=you@sprintly-host          # your SSH target
```

Image names in the registry:

- App images (built by CI): `sprintly-api`, `sprintly-web` — tags `latest` and `sha-<short>`.
- Mirrored bases: `postgres:16-alpine`, `redis:7-alpine`,
  `minio:RELEASE.2024-10-13T13-34-11Z`, `mc:RELEASE.2024-10-08T09-37-26Z`,
  `caddy:2.8-alpine`.

---

## 1. One-time: mirror third-party base images into the registry

Run on the **dual-access machine**. The server can't reach Docker Hub, so the
five base images must be copied into `REGISTRY_HOST` first.

> **Architecture match matters.** The server is `linux/amd64`. If your
> dual-access machine is Apple Silicon / arm64, force amd64 on pull so the
> mirrored image runs on the server. The `--platform` flag below does that.

The registry needs no authentication, so there's no `docker login` — just push.

```sh
export REGISTRY_HOST=docker.netixsystem.com

# Flat-named bases (repo name unchanged):
for img in postgres:16-alpine redis:7-alpine caddy:2.8-alpine; do
  docker pull --platform linux/amd64 "$img"
  docker tag  "$img" "$REGISTRY_HOST/$img"
  docker push "$REGISTRY_HOST/$img"
done

# MinIO images are org-prefixed on Hub (minio/...). Retag to the flat names the
# compose file expects:
docker pull --platform linux/amd64 minio/minio:RELEASE.2024-10-13T13-34-11Z
docker tag  minio/minio:RELEASE.2024-10-13T13-34-11Z "$REGISTRY_HOST/minio:RELEASE.2024-10-13T13-34-11Z"
docker push "$REGISTRY_HOST/minio:RELEASE.2024-10-13T13-34-11Z"

docker pull --platform linux/amd64 minio/mc:RELEASE.2024-10-08T09-37-26Z
docker tag  minio/mc:RELEASE.2024-10-08T09-37-26Z "$REGISTRY_HOST/mc:RELEASE.2024-10-08T09-37-26Z"
docker push "$REGISTRY_HOST/mc:RELEASE.2024-10-08T09-37-26Z"
```

You only repeat this if you bump one of these base-image tags in
`docker-compose.prod.yml`.

---

## 2. One-time: get the app images into the registry

**Preferred — CI.** Merging to `main` runs `release.yml`, which builds
`sprintly-api` and `sprintly-web` for `linux/amd64`, tags each `latest` +
`sha-<short>`, and pushes to `REGISTRY_HOST` with retries.

The registry requires no authentication, so no credential secrets are needed.
Optionally configure in GitHub → *Settings → Secrets and variables → Actions*:

- `vars.REGISTRY_HOST` — optional (defaults to `docker.netixsystem.com`).
- `vars.NEXT_PUBLIC_APP_NAME` — optional (defaults to `Sprintly`).

> **Reachability caveat.** GitHub-hosted runners are on the public internet. If
> `REGISTRY_HOST` is **not** reachable from them, point `release.yml`'s
> `runs-on:` at a **self-hosted runner** on your network. Nothing else changes.

**Fallback — build on the dual-access machine** (if CI can't reach the registry
and you have no self-hosted runner). From a repo checkout:

```sh
export REGISTRY_HOST=docker.netixsystem.com
SHORT=$(git rev-parse --short HEAD)

docker buildx build --platform linux/amd64 \
  -f infra/docker/api.Dockerfile --target runtime \
  -t "$REGISTRY_HOST/sprintly-api:latest" -t "$REGISTRY_HOST/sprintly-api:sha-$SHORT" \
  --load .
docker push "$REGISTRY_HOST/sprintly-api:latest"
docker push "$REGISTRY_HOST/sprintly-api:sha-$SHORT"

docker buildx build --platform linux/amd64 \
  -f infra/docker/web.Dockerfile --target runtime \
  --build-arg NEXT_PUBLIC_API_BASE_URL=/api/v1 \
  --build-arg NEXT_PUBLIC_WS_URL=/ws \
  --build-arg NEXT_PUBLIC_APP_NAME=Sprintly \
  -t "$REGISTRY_HOST/sprintly-web:latest" -t "$REGISTRY_HOST/sprintly-web:sha-$SHORT" \
  --load .
docker push "$REGISTRY_HOST/sprintly-web:latest"
docker push "$REGISTRY_HOST/sprintly-web:sha-$SHORT"
```

> **Why the web build args?** `NEXT_PUBLIC_*` are inlined into the browser
> bundle at build time. We bake **relative** values (`/api/v1`, `/ws`) so a
> single image works behind any host/scheme — the reverse proxy serves the API
> same-origin. Do not bake an absolute host here.

---

## 3. One-time: copy the deploy files to the server

The compose file mounts the Caddyfile via a relative path
(`../docker/caddy/Caddyfile.prod`), so preserve the two-level layout:

```sh
ssh "$SERVER" 'mkdir -p ~/sprintly/infra/compose ~/sprintly/infra/docker/caddy'

scp infra/compose/docker-compose.prod.yml "$SERVER":~/sprintly/infra/compose/
scp infra/docker/caddy/Caddyfile.prod     "$SERVER":~/sprintly/infra/docker/caddy/
# Seed the env file from the template (you fill it in next step):
scp .env.prod.example                     "$SERVER":~/sprintly/infra/compose/.env
```

All later commands run on the server from `~/sprintly/infra/compose`.

---

## 4. One-time: generate secrets on the server (`/dev/urandom`, no `openssl`)

Alpine ships no `openssl`. Generate everything from `/dev/urandom` with busybox
tools. **You** run this on the server — no secret is ever generated, seen, or
committed by anyone else.

```sh
cd ~/sprintly/infra/compose

# 64 random bytes base64 (JWT); 32 random bytes base64 (vault); URL-safe
# alphanumeric passwords (no '/', '+', '@' to keep DATABASE_URL clean).
gen64() { head -c 64 /dev/urandom | base64 | tr -d '\n'; }
gen32() { head -c 32 /dev/urandom | base64 | tr -d '\n'; }
genpw() { LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 32; }

JWT=$(gen64); VAULT=$(gen32); PGPW=$(genpw); MINIOPW=$(genpw)

# Write them into .env, keyed by variable name (busybox sed):
sed -i "s|^SPRINTLY_JWT_SECRET=.*|SPRINTLY_JWT_SECRET=$JWT|"                 .env
sed -i "s|^SPRINTLY_VAULT_MASTER_KEY=.*|SPRINTLY_VAULT_MASTER_KEY=$VAULT|"   .env
sed -i "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=$PGPW|"                    .env
sed -i "s|^DATABASE_URL=.*|DATABASE_URL=postgres://sprintly:$PGPW@postgres:5432/sprintly|" .env
sed -i "s|^MINIO_ROOT_PASSWORD=.*|MINIO_ROOT_PASSWORD=$MINIOPW|"             .env

# Lock it down.
chmod 600 .env
```

Then edit the non-secret settings in `.env` by hand:

- `REGISTRY_HOST` — confirm it's `docker.netixsystem.com`.
- `SPRINTLY_PUBLIC_URL` and `MINIO_PUBLIC_ENDPOINT` — the URL users hit.
- `SPRINTLY_HTTP_PORT` — published HTTP port (default `80`).
- `SPRINTLY_OPEN_SIGNUP=true` for first boot (see §6), then flip to `false`.
- If you changed `POSTGRES_USER`/`POSTGRES_DB`, update `DATABASE_URL` to match.

> Sanity check without booting: `grep -c __GENERATE_ON_SERVER__ .env` must print
> `0`. Any leftover placeholder means a secret wasn't filled in.

The registry needs no login, so the server can pull straight away. (If the
registry is served over plain HTTP rather than HTTPS, add it to the Docker
daemon's `insecure-registries` in `/etc/docker/daemon.json` and restart Docker —
this is the one registry-side setup the daemon needs.)

---

## 5. Deploy (first time and every time)

From `~/sprintly/infra/compose` on the server:

```sh
docker compose -f docker-compose.prod.yml --env-file .env pull
docker compose -f docker-compose.prod.yml --env-file .env up -d
```

That's the whole per-deploy flow. `pull` fetches the images named in `.env`
(`IMAGE_TAG`, default `latest`) from the registry; `up -d` recreates changed
containers. The API applies any pending migrations at startup before it accepts
traffic (idempotent — see §7).

Check status / logs:

```sh
docker compose -f docker-compose.prod.yml --env-file .env ps
docker compose -f docker-compose.prod.yml --env-file .env logs -f --tail=200 api
```

Sprintly is then reachable at `http://<server>:<SPRINTLY_HTTP_PORT>/`.

---

## 6. One-time: seed + first admin

The seed is **idempotent** (every write is guarded / `ON CONFLICT DO NOTHING`),
so it's safe to run once or many times. Run it once after the first deploy:

```sh
docker compose -f docker-compose.prod.yml --env-file .env run --rm seed
```

This ensures the demo admin exists (`demo@sprintly.local` / `sprintly`). **Log
in and change that password immediately**, or register your own admin while
`SPRINTLY_OPEN_SIGNUP=true`, then set `SPRINTLY_OPEN_SIGNUP=false` in `.env` and
re-run the deploy in §5 to close open registration.

---

## 7. Migrations

Handled automatically: the API runs `sqlx migrate` at startup (controlled by
`SPRINTLY_AUTO_MIGRATE`, default `true`) before binding. The migration set is
compiled into the binary, and SQLx records applied migrations in
`_sqlx_migrations`, so:

- A fresh DB is fully migrated on first boot.
- A redeploy applies only new migrations; a restart with no new migrations is a
  no-op.

To manage migrations out of band, set `SPRINTLY_AUTO_MIGRATE=false` and run
`docker compose ... run --rm api migrate` yourself.

---

## 8. Rollback

Images are immutable per commit. To roll back, pin `IMAGE_TAG` to a known-good
`sha-<short>` in `.env` and redeploy:

```sh
sed -i "s|^IMAGE_TAG=.*|IMAGE_TAG=sha-abc1234|" .env
docker compose -f docker-compose.prod.yml --env-file .env pull
docker compose -f docker-compose.prod.yml --env-file .env up -d
```

> A rollback does **not** revert database migrations. If a release added a
> migration, rolling the image back may leave newer schema in place. Test
> migrations on staging before shipping anything you might need to undo.

---

## 9. Caveats — read before going live

**Plain HTTP / TLS.** An air-gapped box can't complete a public ACME (Let's
Encrypt) challenge, so Caddy serves **plain, unencrypted HTTP** on the published
port. That's acceptable only on a trusted, isolated network. To add TLS without
public ACME, do one of (details in
[`Caddyfile.prod`](../infra/docker/caddy/Caddyfile.prod)):

- terminate TLS at an upstream appliance/LB you control and forward cleartext;
- mount an internally-issued cert+key and switch the site to `https://<host>` +
  a `tls <cert> <key>` directive (and publish `443` in the compose `ports:`);
- use Caddy's internal CA (`tls internal`) and distribute its root to clients.

**Email.** With `SPRINTLY_SMTP_URL` unset, outbound mail is **logged, not
sent** — password-reset and invite emails will not be delivered. An air-gapped
host typically can't reach public SMTP either; point `SPRINTLY_SMTP_URL` at an
**internal relay** reachable from the box if you need real delivery. Prefer
`smtps://` (implicit TLS) or `smtp://...:587` (STARTTLS).

**Secrets.** Never commit `.env`. It's gitignored. Secrets are generated on the
server (§4) and never leave it. If you must rotate `SPRINTLY_VAULT_MASTER_KEY`,
note that existing vault ciphertext was encrypted under the old key — rotate via
the app's key-version mechanism, not by editing `.env` blindly.
