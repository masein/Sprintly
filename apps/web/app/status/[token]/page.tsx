"use client";

// Public, unauthenticated read-only project status (F18). No AppShell, no
// session — just a whitelisted summary. Renders an honest "off" state when the
// token is wrong or disabled.

import { useEffect, useState } from "react";
import { useParams } from "next/navigation";
import { getPublicView, type PublicView } from "@/lib/publicStatus";

export default function PublicStatusPage() {
  const params = useParams<{ token: string }>();
  const token = params?.token ?? "";
  const [view, setView] = useState<PublicView | null>(null);
  const [state, setState] = useState<"loading" | "ok" | "off">("loading");

  useEffect(() => {
    let alive = true;
    getPublicView(token)
      .then((v) => alive && (setView(v), setState("ok")))
      .catch(() => alive && setState("off"));
    return () => {
      alive = false;
    };
  }, [token]);

  return (
    <main className="mx-auto flex min-h-screen max-w-2xl flex-col justify-center gap-8 px-6 py-20">
      {state === "loading" && (
        <div className="mono text-sm text-chrome-dim">fetching the latest…</div>
      )}

      {state === "off" && (
        <div className="space-y-2">
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · status
          </div>
          <h1 className="text-2xl font-semibold">Nothing to see here.</h1>
          <p className="text-sm text-chrome-dim">
            This status page is switched off, or the link isn&apos;t right.
          </p>
        </div>
      )}

      {state === "ok" && view && (
        <>
          <header className="space-y-1">
            <div className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              {view.project_key} · live status
              <span className="inline-block h-1.5 w-1.5 rounded-full bg-emerald-400" aria-hidden />
            </div>
            <h1 className="text-3xl font-semibold">{view.project_name}</h1>
          </header>

          {view.sprint ? (
            <section className="space-y-2 rounded-lg border border-white/10 bg-ink-subtle p-4">
              <div className="flex items-baseline justify-between">
                <h2 className="text-lg font-medium">{view.sprint.name}</h2>
                <span className="mono text-xs text-chrome-dim">
                  {view.sprint.done}/{view.sprint.total} done
                </span>
              </div>
              <div className="h-2 w-full overflow-hidden rounded-full bg-white/10">
                <div
                  className="h-full rounded-full bg-accent transition-all"
                  style={{ width: `${view.sprint.percent}%` }}
                  role="progressbar"
                  aria-valuenow={view.sprint.percent}
                  aria-valuemin={0}
                  aria-valuemax={100}
                />
              </div>
              <div className="mono text-[11px] text-chrome-dim">
                {view.sprint.percent}% · {fmtDate(view.sprint.starts_at)} →{" "}
                {fmtDate(view.sprint.ends_at)}
              </div>
            </section>
          ) : (
            <p className="mono text-sm text-chrome-dim">No active sprint right now.</p>
          )}

          <section className="space-y-2">
            <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">board</h2>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
              {view.columns.map((c) => (
                <div
                  key={c.name}
                  className="rounded-lg border border-white/10 bg-ink-subtle p-3 text-center"
                >
                  <div className="text-2xl font-semibold">{c.count}</div>
                  <div className="mono mt-1 truncate text-[11px] text-chrome-dim">{c.name}</div>
                </div>
              ))}
            </div>
          </section>

          <footer className="mono text-[11px] text-chrome-dim">
            read-only · counts only, no private details · powered by Sprintly
          </footer>
        </>
      )}
    </main>
  );
}

function fmtDate(iso: string): string {
  return iso.slice(0, 10);
}
