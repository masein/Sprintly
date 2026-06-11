"use client";

// /docs — the in-app docs page. Visiting it once awards RTFM.
//
// We trigger on mount, idempotently. The server collapses repeat awards.

import { useEffect } from "react";
import Link from "next/link";
import { Book, KeyRound, ListChecks, Vault, Coffee, Sparkles } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Sprint } from "@/components/Sprint";
import { triggerRtfm } from "@/lib/achievements";

export default function DocsPage() {
  useEffect(() => {
    void triggerRtfm().catch(() => {});
  }, []);

  return (
    <AppShell>
      <div className="grid grid-cols-1 gap-8 lg:grid-cols-[1fr_180px]">
        <article className="min-w-0 space-y-6">
          <header>
            <div className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
              <Book size={11} /> sprintly · docs
            </div>
            <h1 className="text-3xl font-semibold">Read the manual.</h1>
            <p className="mt-1 text-sm text-chrome-dim">
              Short. Honest. Updated as we go.
            </p>
          </header>

          <Section icon={KeyRound} title="Shortcuts">
            <ul className="mono space-y-1 text-sm">
              <li><kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd> — command palette</li>
              <li><kbd>/</kbd> — open palette in search mode</li>
              <li><kbd>?</kbd> — shortcut help</li>
              <li><kbd>c</kbd> — new card in leftmost column</li>
              <li><kbd>g p</kbd> projects · <kbd>g m</kbd> my tasks · <kbd>g d</kbd> my day · <kbd>g s</kbd> settings</li>
              <li><kbd>:q</kbd> close modal · <kbd>:wq</kbd> save &amp; close</li>
            </ul>
          </Section>

          <Section icon={Vault} title="Vault">
            <p>
              Secrets are encrypted with a per-project key derived from a master
              you set in <span className="mono">SPRINTLY_VAULT_MASTER_KEY</span>.
              Reveals are <span className="mono">rate-limited</span> (10/hr) and
              audit-logged. Clipboard copies auto-clear after 30 seconds. Don&apos;t
              put the actual secret in the description field — there&apos;s a field
              for that further down.
            </p>
          </Section>

          <Section icon={ListChecks} title="Labels & custom fields">
            <p>
              Labels are free-form tags with a per-project palette (the{" "}
              <span className="mono">labels</span> button on the project page
              maps a name to a colour — the name is always shown, the colour is
              decoration). Custom fields are typed:{" "}
              <span className="mono">text · number · select · date</span>,
              defined per project under <span className="mono">fields</span>,
              set on each task&apos;s sidebar. Values are validated against the
              type, so a date field won&apos;t quietly hold &quot;next sprint,
              probably&quot;.
            </p>
            <p>
              Both filter the board:{" "}
              <span className="mono">label:backend</span> and{" "}
              <span className="mono">field:severity=high</span> chips stack,
              and every predicate must match. Field values also feed search
              (<kbd>⌘K</kbd>).
            </p>
          </Section>

          <Section icon={Coffee} title="Time tracking">
            <p>
              One running timer per person at a time. Manual entries land in
              the same place. Weekly timesheets submit-then-approve; an
              approved week locks logs in its range. Monthly payroll
              aggregates billable minutes × your hourly rate (cents math, no
              floats). PDF + CSV exports.
            </p>
          </Section>

          <Section icon={Sparkles} title="Achievements">
            <p>
              Catalog of eight, including the one you just earned by reading
              this. None of them reward longer hours. The
              <span className="mono"> Coffee Meter</span> in the header is for
              you, not your manager. Managers don&apos;t see other people&apos;s meters.
            </p>
            <p className="mono mt-2 text-xs text-chrome-dim">
              psst — try typing <span className="text-chrome">konami</span>{" "}
              in the command palette.
            </p>
          </Section>

          <p className="mono text-[11px] text-chrome-dim">
            That&apos;s it. There&apos;s no chapter 12.
          </p>
        </article>

        <aside className="hidden lg:block">
          <div className="rounded-lg border border-white/10 bg-ink-subtle p-3">
            <Sprint mood="proud" size={140} className="mx-auto" />
            <p className="mono mt-2 text-center text-[10px] text-chrome-dim">
              you read the manual.<br />achievement: RTFM.
            </p>
            <Link
              href="/me/achievements"
              className="mono mt-3 block text-center text-[11px] text-accent hover:underline"
            >
              → see all
            </Link>
          </div>
        </aside>
      </div>
    </AppShell>
  );
}

function Section({
  icon: Icon,
  title,
  children,
}: {
  icon: React.ComponentType<{ size?: string | number }>;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-white/10 bg-ink-subtle p-4">
      <h2 className="mono mb-2 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        <Icon size={11} /> {title}
      </h2>
      <div className="space-y-2 text-sm leading-relaxed text-chrome">
        {children}
      </div>
    </section>
  );
}
