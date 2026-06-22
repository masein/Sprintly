"use client";

// /docs — the in-app docs page. Visiting it once awards RTFM.
//
// We trigger on mount, idempotently. The server collapses repeat awards.

import { useEffect } from "react";
import Link from "next/link";
import { ArrowDownUp, Book, FileStack, GanttChartSquare, GitBranch, KeyRound, ListChecks, ListTree, LogIn, Receipt, Rows3, Share2, ShieldCheck, Smartphone, TerminalSquare, Vault, Webhook, Coffee, Sparkles } from "lucide-react";
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
            <p className="mt-2">
              Quick-add works inside a sprint too: the{" "}
              <span className="mono">+ add tasks</span> box finds existing tasks
              as you type, and typing a brand-new title then{" "}
              <kbd>↵</kbd> creates it straight into the sprint — the field clears
              and keeps focus, so you can plan a whole sprint without leaving the
              keyboard.
            </p>
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

          <Section icon={FileStack} title="Templates, recurrence & the backlog">
            <p>
              Save a task skeleton as a <span className="mono">template</span>{" "}
              (from the project header) and spawn a task from it whenever. Give
              one a <span className="mono">repeat</span> —{" "}
              <span className="mono">daily / weekly / monthly</span> — and a
              background worker drops a fresh task each interval (it catches up
              without spamming if it falls behind). The{" "}
              <span className="mono">backlog</span> (everything with no sprint)
              has multi-select: tick a few, then assign, drop into a sprint, or
              delete them in one action.
            </p>
          </Section>

          <Section icon={GanttChartSquare} title="Roadmap & timeline">
            <p>
              Each project has a <span className="mono">timeline</span> (linked
              from the board header): group tasks into <span className="mono">epics
              </span> — coloured, date-ranged bars that show{" "}
              <span className="mono">done/total</span> progress — and drop{" "}
              <span className="mono">milestones</span> as dated markers. Assign a
              task to an epic from the task&apos;s sidebar. Dragging a bar to
              reschedule is a v2 idea; for now, edit an epic&apos;s dates in its
              row and the bar moves.
            </p>
          </Section>

          <Section icon={ListTree} title="Subtasks">
            <p>
              Break a task down from its <span className="mono">subtasks</span>{" "}
              panel. A subtask is a real task — it has its own key, status, and
              detail page — but it lives <em>under</em> its parent: it does{" "}
              <span className="mono">not</span> get its own card on the board or
              a row in the backlog, and it doesn&apos;t inflate column or sprint
              counts. Its detail page breadcrumbs back to the parent
              (<span className="mono">↳ QAV-1</span>). It&apos;s still findable
              by key/title in search and shows up in your task list — it just
              stops masquerading as independent top-level work.
            </p>
          </Section>

          <Section icon={Rows3} title="Board views & swimlanes">
            <p>
              Filter the board with chips, then group it into swimlanes by{" "}
              <span className="mono">assignee</span>,{" "}
              <span className="mono">label</span>, or{" "}
              <span className="mono">priority</span> from the{" "}
              <span className="mono">swimlanes</span> control. Save a filter +
              grouping as a named <span className="mono">view</span> to reopen
              later; tick <span className="mono">shared</span> and the rest of
              the project can pick it too (yours to edit, theirs to use). In a
              grouped view cards still drag between columns within their lane —
              changing lane means changing the card&apos;s assignee, label, or
              priority on the card itself.
            </p>
            <p>
              The <span className="mono">scope</span> control sets which sprint
              the board shows. While a sprint is running the board opens on{" "}
              <span className="mono">active sprint</span> so you see only what
              you committed to; switch to <span className="mono">all tasks</span>{" "}
              for the whole project (sprint cards plus the backlog), or pick a
              specific sprint to look back. With no sprint running the control
              reads <span className="mono">all tasks (no active sprint)</span>.
              Column counts follow the scope, and your choice is remembered per
              project.
            </p>
            <p>
              Click anywhere on a card to open it — the small{" "}
              <span className="mono">KEY</span> link still works, but the whole
              card is a target now (a real drag still moves the card; a click
              that doesn&apos;t travel opens it). The{" "}
              <span className="mono">+ add card</span> box stays available in
              swimlane mode and drops the new card into the lane you add it from
              (its assignee / label / priority). <kbd>Esc</kbd> dismisses any
              inline editor — add-card, add-column, a column rename — the same as
              the <span className="mono">:q cancel</span> control. Change a
              card&apos;s <span className="mono">status</span> from the task
              detail&apos;s <span className="mono">details</span> panel: the
              dropdown lists the board&apos;s real columns, so moving status there
              moves the card on the board too.
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
            <p>
              Apply them from a task&apos;s <span className="mono">details</span>{" "}
              panel: the <span className="mono">labels</span> row adds/removes
              from the project palette (chips carry colour + text on the task and
              its board card), and the <span className="mono">assignee</span>{" "}
              picker hands the task to any project member — or{" "}
              <span className="mono">unassigned</span>. Assigning notifies the new
              owner and shows their avatar on the card.
            </p>
          </Section>

          <Section icon={Smartphone} title="Mobile & offline">
            <p>
              The board, task detail, and the rest are usable down to a phone
              screen — columns scroll sideways, the project toolbar wraps.
              Sprintly is also an installable <span className="mono">PWA</span>:
              add it to your home screen and it opens standalone. The app shell
              is cached, so a dropped connection shows a clean{" "}
              <span className="mono">offline</span> screen and a banner instead
              of a broken page — your data syncs again the moment you&apos;re
              back online (we never cache the API, so nothing goes stale).
            </p>
          </Section>

          <Section icon={Share2} title="Public status pages">
            <p>
              A project lead can flip on a{" "}
              <span className="mono">public status</span> page (from the project
              header): it mints a tokenised, unauthenticated URL at{" "}
              <span className="mono">/status/&lt;token&gt;</span> showing the
              active sprint&apos;s progress and per-column task{" "}
              <span className="mono">counts</span> — and nothing else. No task
              titles, assignees, labels, comments, custom fields, or anything
              vault-adjacent ever leave the building. Off by default; turning it
              off invalidates the link immediately.
            </p>
          </Section>

          <Section icon={ArrowDownUp} title="Import & export">
            <p>
              From a project&apos;s header (<span className="mono">import / export</span>):
              bring a board in from a <span className="mono">Trello</span> JSON export
              or a <span className="mono">CSV</span> (a <span className="mono">name</span>{" "}
              column, plus optional <span className="mono">description / list / labels</span>) —
              cards become tasks, lists become columns, labels are created as
              needed. Import always <span className="mono">previews</span> first: a
              dry-run shows exactly what would be created (it&apos;s the real
              resolution, rolled back) before you commit.
            </p>
            <p>
              <span className="mono">Jira</span> gets a first-class importer: drop
              in a Jira <span className="mono">&ldquo;Export Excel CSV (all
              fields)&rdquo;</span> export and it&apos;s auto-detected (no need to
              pick a format) and mapped richly. A re-import of the same export{" "}
              <span className="text-chrome">updates</span> cards instead of
              duplicating them — they&apos;re matched by Jira issue key.
            </p>
            <div className="overflow-x-auto">
              <table className="mono w-full text-xs">
                <thead>
                  <tr className="text-chrome-dim">
                    <th className="py-1 pr-4 text-left font-normal">Jira</th>
                    <th className="py-1 text-left font-normal">Sprintly</th>
                  </tr>
                </thead>
                <tbody className="text-chrome-dim">
                  {[
                    ["Summary / Description", "title / description"],
                    ["Status", "board column (To do / In progress / Done…)"],
                    ["Labels (repeated columns)", "labels"],
                    ["Assignee (email, then name)", "user — unmatched → unassigned + warning"],
                    ["Priority Highest…Lowest", "p0 · p1 · p2 · p3"],
                    ["Issue Type (Task → feature)", "feature · bug · chore · spike · incident"],
                    ["Epic Link / Parent epic", "epic (created, task’s epic set)"],
                    ["Sub-task + Parent", "subtask nested under its parent (warns if absent)"],
                    ["Sprint (+ window / state)", "sprint (created with dates + open/closed where given)"],
                    ["Story Points", "a “Story Points” number custom field"],
                    ["Comment (date;author;body)", "task comment (author matched, else attributed in body)"],
                    ["Due date", "due date"],
                    ["Issue key", "external ref → idempotent re-import"],
                  ].map(([j, s]) => (
                    <tr key={j} className="border-t border-white/5">
                      <td className="py-1 pr-4 align-top text-chrome">{j}</td>
                      <td className="py-1 align-top">{s}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p>
              A Jira import is a <span className="text-chrome">historical</span>{" "}
              migration, so imported sprints land <span className="mono">completed</span>{" "}
              — no &ldquo;start sprint&rdquo; button — carrying their real
              start/end dates and open/closed state when the export encodes them.
              The single in-flight sprint (active in Jira, or the most-recent
              open-ended one) stays <span className="mono">active</span>.
            </p>
            <p>
              <span className="text-chrome">Create missing users</span> (a checkbox
              on the Jira import, off by default): for each assignee, reporter, or
              watcher with no Sprintly match, it provisions an account (display
              name, derived handle, synthetic{" "}
              <span className="mono">@jira-import.local</span> email if there
              isn&apos;t one), adds them to the project, and wires them up —
              assignee, reporter, and watchers on each card. Each is set with an operator-supplied{" "}
              <span className="mono">temporary password</span> and a{" "}
              <span className="mono">force-reset</span> flag — at first login they
              get a challenge to set their own password before any session is
              issued (the temp password is never logged). Leave the box off to
              keep today&apos;s match-only behaviour (unmatched people are warned
              and left unassigned).
            </p>
            <p>
              Export the other way: a <span className="mono">JSON</span> bundle
              (tasks with comments and an attachment manifest — metadata, not the
              bytes) or a flat task <span className="mono">CSV</span>.
            </p>
          </Section>

          <Section icon={Receipt} title="Billing & invoices">
            <p>
              Admins manage <span className="mono">clients</span> in{" "}
              <span className="mono">/billing</span> and link each project to
              one. Generating an invoice for a client + date range rolls up the{" "}
              <span className="mono">billable</span> time logged on those
              projects, one line per project + contributor, priced at each
              person&apos;s configured hourly rate (cents math, no floats — the
              PDF total equals the sum of the lines).
            </p>
            <p>
              Export to <span className="mono">PDF</span> or{" "}
              <span className="mono">CSV</span>, then walk it{" "}
              <span className="mono">draft → sent → paid</span>. Draft invoices
              can be deleted; once sent or paid they&apos;re kept as a record.
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

          <Section icon={GitBranch} title="Git integration">
            <p>
              Connect a repo from the project header (<span className="mono">git</span>{" "}
              button): pick GitHub, GitLab, or Gitea, get a webhook URL +
              secret (shown once), paste both into the provider. From then on,
              commits, branches and PRs that mention a task key —{" "}
              <span className="mono">DEMO-1</span> in a commit message or PR
              title — link themselves to the task, and merging a linked PR
              moves it to done.
            </p>
            <p>
              Add a provider API token and flip <span className="mono">status</span>{" "}
              to push task state back as commit statuses: done is{" "}
              <span className="mono">success</span>, everything else{" "}
              <span className="mono">pending</span>. Tokens and secrets are
              vault-encrypted; linking is scoped to the connected project.
            </p>
            <p>
              The other direction too: CI check and pipeline results land on
              the task as a pass / fail / pending chip on the linked PR — icon
              and label, not just a colour.
            </p>
          </Section>

          <Section icon={Webhook} title="Webhooks">
            <p>
              Per-project, from the <span className="mono">webhooks</span> button
              on the board. A <span className="mono">generic</span> target gets
              signed JSON (verify <span className="mono">X-Sprintly-Signature:
              sha256=…</span> against your secret); <span className="mono">slack
              </span> and <span className="mono">discord</span> targets get a
              formatted message posted to their webhook URL. Pick which board
              events fire it, hit <span className="mono">send test</span>, and
              every attempt — code, retry, error — shows in the deliveries list.
            </p>
          </Section>

          <Section icon={LogIn} title="Single sign-on (OIDC)">
            <p>
              Point Sprintly at your identity provider — Authentik, Keycloak,
              Google, anything that speaks <span className="mono">OIDC</span> —
              with <span className="mono">SPRINTLY_OIDC_ISSUER</span>,{" "}
              <span className="mono">_CLIENT_ID</span> and{" "}
              <span className="mono">_CLIENT_SECRET</span>. A{" "}
              <span className="mono">$ sso --login</span> button then appears on
              the sign-in page. First login creates an account (or links to an
              existing one by verified email); the flow uses{" "}
              <span className="mono">auth-code + PKCE</span> with state and nonce
              checks throughout.
            </p>
            <p>
              Restrict who can get in with{" "}
              <span className="mono">SPRINTLY_OIDC_ALLOWED_DOMAINS</span>{" "}
              (comma-separated email domains). Password login keeps working
              alongside SSO unless you set{" "}
              <span className="mono">SPRINTLY_LOCAL_LOGIN_DISABLED=true</span> to
              go SSO-only.
            </p>
          </Section>

          <Section icon={ShieldCheck} title="Two-factor auth">
            <p>
              Turn on <span className="mono">two-factor</span> in{" "}
              <span className="mono">/settings</span>: scan the QR with any
              authenticator app (or type the setup key), confirm one code, and
              save the <span className="mono">recovery codes</span> we show once.
              After that, signing in asks for a 6-digit code on top of your
              password. Codes are standard <span className="mono">TOTP</span> —
              SHA1, 6 digits, 30s — so Google Authenticator, 1Password, Authy,
              anything works, with ±30s of clock slack.
            </p>
            <p>
              Lost your phone? Each recovery code works exactly once, at the
              code prompt or to turn 2FA off. Wrong codes are rate-limited.
              Admins can set <span className="mono">SPRINTLY_REQUIRE_2FA</span>{" "}
              to nudge everyone to enrol.
            </p>
          </Section>

          <Section icon={TerminalSquare} title="API tokens">
            <p>
              Mint one in <span className="mono">/settings</span>, send{" "}
              <span className="mono">Authorization: Bearer slt_…</span>, and
              the REST API is yours — no cookies, no CSRF dance. Tokens are
              read-only unless you grant write, the secret is shown exactly
              once (we store a hash), and revoking kills it on the next
              request. Optional expiry for the cautious.
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
