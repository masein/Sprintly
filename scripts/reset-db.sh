#!/usr/bin/env bash
# Drop and recreate the dev database. Convenience wrapper around `just reset`.
# Don't run this against anything you care about.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "  Wiping volumes…"
docker compose -f infra/compose/docker-compose.yml --env-file .env down -v

echo "  Recreating stack…"
just up
