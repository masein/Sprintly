# ─────────────────────────────────────────────────────────────────────────────
# Sprintly — task runner. Run `just` to see all commands.
# Pick one and stick with it: we use `just`, not `make`. The whole stack lives
# in `infra/compose/docker-compose.yml`. The API/web Dockerfiles live in
# `infra/docker/`. There is no magic outside this file.
# ─────────────────────────────────────────────────────────────────────────────

set dotenv-load := true
set positional-arguments
set shell := ["bash", "-cu"]

compose := "docker compose -f infra/compose/docker-compose.yml --env-file .env"

# Default: list available recipes.
default:
    @just --list --unsorted

# Bring the whole dev stack up. Idempotent.
up:
    {{compose}} up -d --build
    @echo ""
    @echo "  Sprintly is booting."
    @echo "  Web:    http://localhost:8080"
    @echo "  API:    http://localhost:8080/api/v1/healthz"
    @echo "  MinIO:  http://localhost:9001  (console)"
    @echo ""
    @echo "  Tail logs with:  just logs"

# Alias for new contributors.
dev: up

# Tear it down. Volumes preserved.
down:
    {{compose}} down

# Nuke containers AND volumes. Use sparingly.
reset:
    {{compose}} down -v
    @echo "Volumes wiped. Next 'just up' starts from a fresh DB."

# Tail logs from everything.
logs:
    {{compose}} logs -f --tail=200

# Tail logs from a single service: `just log api`
log service:
    {{compose}} logs -f --tail=200 {{service}}

# Apply migrations against the running dev DB.
migrate:
    {{compose}} exec api sqlx migrate run

# Create a new migration: `just migrate-new add_users_table`
migrate-new name:
    cd apps/api && sqlx migrate add -r {{name}}

# Regenerate the SQLx offline query cache. Run this whenever you change a
# query! macro. Commits to apps/api/.sqlx/ and lets prod docker builds run
# without a live DB.
sqlx-prepare:
    {{compose}} exec api cargo sqlx prepare --workspace -- --bin sprintly-api

# Seed the database with demo data (M1 stub; fleshed out in later milestones).
seed:
    {{compose}} exec api /usr/local/bin/sprintly-seed

# Run all backend tests inside the container.
test:
    {{compose}} exec api cargo test --workspace --locked

# Run frontend tests.
test-web:
    {{compose}} exec web pnpm test

# Run the Playwright smoke against a running stack. First time only:
#   pnpm i && pnpm e2e:install
e2e:
    pnpm --filter @sprintly/e2e test

# Run a single e2e spec.
e2e-spec spec:
    pnpm --filter @sprintly/e2e test {{spec}}

# Lint everything.
lint:
    {{compose}} exec api cargo clippy --workspace --all-targets -- -D warnings
    {{compose}} exec api cargo fmt --all -- --check
    {{compose}} exec web pnpm lint

# Format everything.
fmt:
    cd apps/api && cargo fmt --all
    cd apps/web && pnpm format

# Open a psql shell on the dev DB.
psql:
    {{compose}} exec postgres psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"

# Open a redis-cli shell.
redis:
    {{compose}} exec redis redis-cli

# Open a shell inside the API container.
sh-api:
    {{compose}} exec api bash

# Open a shell inside the web container.
sh-web:
    {{compose}} exec web sh

# Generate the OpenAPI spec + TS types into packages/shared-types.
gen-types:
    {{compose}} exec api /usr/local/bin/sprintly-api --emit-openapi > apps/api/openapi.json
    cd apps/web && pnpm openapi-typescript ../api/openapi.json -o ../../packages/shared-types/src/generated.ts

# Quick status of the stack.
status:
    {{compose}} ps
