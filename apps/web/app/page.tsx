// Landing page. Shows the boot status + a session badge that reflects auth.
// The real authed home page (dashboard) lands in M6.

import Link from "next/link";
import { SessionBadge } from "@/components/SessionBadge";
import { Sprint } from "@/components/Sprint";
import { APP_VERSION } from "@/lib/version";

export default function Home() {
  const apiBase =
    process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

  return (
    <main className="mx-auto flex min-h-screen max-w-3xl flex-col justify-center gap-10 px-6 py-20">
      <div className="flex items-center justify-between">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · v{APP_VERSION}
        </div>
        <SessionBadge />
      </div>

      <Sprint mood="happy" size={96} />

      <header className="space-y-3">
        <h1 className="text-5xl font-semibold leading-tight">
          Project management,{" "}
          <span className="text-accent">
            but for people who read changelogs.
          </span>
        </h1>
        <p className="max-w-xl text-chrome-dim">
          Self-hosted. Dockerized. Dark by default. Auth + projects shipped.
          Tasks, sprints, time tracking, and the vault land in the next
          milestones.
        </p>
        <div className="mono pt-2 text-xs">
          <Link href="/projects" className="text-accent hover:underline">
            → your projects
          </Link>
        </div>
      </header>

      <section className="rounded-lg border border-white/10 bg-ink-subtle p-6">
        <h2 className="mono mb-4 text-sm uppercase tracking-widest text-chrome-dim">
          $ curl healthz
        </h2>
        <pre className="mono overflow-x-auto text-sm">
          <code>{`GET ${apiBase}/healthz
GET ${apiBase}/readyz
GET ${apiBase}/users/me     # requires auth`}</code>
        </pre>
      </section>

      <footer className="mono text-xs text-chrome-dim">
        nudging electrons…
      </footer>
    </main>
  );
}
