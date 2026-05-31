// Tiny health endpoint for the docker-compose healthcheck on the web service.
// Not a substitute for the Rust API's /healthz — this only proves Next is up.

import { NextResponse } from "next/server";

export const dynamic = "force-dynamic";

export function GET() {
  return NextResponse.json({ status: "alive", service: "sprintly-web" });
}
