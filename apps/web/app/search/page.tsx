"use client";

// /search — query-language search over every task you can see, with saved
// queries. QA report 3: "Add JQL search and can save the templates."
//
// Three things carry the page: the editor (with the server's parse error
// pointed at the character it choked on), the saved-query rail, and a results
// table that links straight into the tasks. The query lives in `?jql=` so a
// result set is a URL you can paste to someone.

import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import {
  BookmarkPlus,
  ChevronLeft,
  ChevronRight,
  Pencil,
  Play,
  Search,
  Trash2,
  Users,
} from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AppShell } from "@/components/AppShell";
import { LoadError } from "@/components/LoadError";
import {
  JQL_EXAMPLES,
  JQL_FIELDS,
  createSavedQuery,
  deleteSavedQuery,
  listSavedQueries,
  runJql,
  updateSavedQuery,
  type JqlHit,
  type SavedQuery,
} from "@/lib/jql";
import type { ApiError } from "@/lib/api";

const PAGE = 50;

const PRIORITY_COLOR: Record<string, string> = {
  p0: "#ef4444",
  p1: "#f59e0b",
  p2: "#a3a3a3",
  p3: "#6b7280",
};

// `useSearchParams` bails out of static prerendering, so the query-reading
// half sits behind a Suspense boundary and the shell renders immediately.
export default function SearchPage() {
  return (
    <Suspense
      fallback={
        <AppShell>
          <p className="mono text-xs text-chrome-dim">loading search…</p>
        </AppShell>
      }
    >
      <SearchInner />
    </Suspense>
  );
}

function SearchInner() {
  const router = useRouter();
  const params = useSearchParams();
  const qc = useQueryClient();

  const urlJql = params.get("jql") ?? "";
  // `draft` is what's in the box; `active` is what was actually run. Typing
  // shouldn't fire a query per keystroke against a language that can be
  // half-written and meaningless.
  const [draft, setDraft] = useState(urlJql);
  const [active, setActive] = useState(urlJql);
  const [offset, setOffset] = useState(0);
  const [saveOpen, setSaveOpen] = useState(false);

  // Back/forward and pasted links both arrive as a URL change.
  useEffect(() => {
    setDraft(urlJql);
    setActive(urlJql);
    setOffset(0);
  }, [urlJql]);

  const results = useQuery({
    queryKey: ["jql", active, offset],
    queryFn: () => runJql(active, PAGE, offset),
  });

  const saved = useQuery({ queryKey: ["saved-queries"], queryFn: listSavedQueries });

  const run = useCallback((text: string) => {
    const next = text.trim();
    setActive(next);
    setOffset(0);
    // Sync the URL without navigating: `router.replace` would re-render the
    // route (and typed routes don't accept a computed path anyway), while the
    // history API just rewrites the address bar — which is all we want, so the
    // result set stays a link you can paste to someone.
    const qs = next ? `?jql=${encodeURIComponent(next)}` : "";
    window.history.replaceState(null, "", `/search${qs}`);
  }, []);

  const error = results.error as unknown as ApiError | undefined;
  // A 400 is the query's fault, not the page's — show it under the box and
  // keep whatever results were already there.
  const parseError = error?.status === 400 ? error.message : null;

  if (error && error.status === 401) {
    router.push("/login");
    return null;
  }

  const total = results.data?.total ?? 0;
  const items = results.data?.items ?? [];
  const from = total === 0 ? 0 : offset + 1;
  const to = Math.min(offset + PAGE, total);

  return (
    <AppShell>
      <div className="space-y-6">
        <header className="space-y-1">
          <nav aria-label="breadcrumb" className="mono text-xs text-chrome-dim">
            <Link href="/me/day" className="hover:text-chrome">
              sprintly
            </Link>
            <span className="px-1">·</span>
            <span className="text-chrome">search</span>
          </nav>
          <h1 className="mono flex items-center gap-2 text-lg">
            <Search size={16} className="text-accent" />
            search
          </h1>
          <p className="mono text-xs text-chrome-dim">
            Query every task you can see. An empty query means everything.
          </p>
        </header>

        <QueryEditor
          value={draft}
          onChange={setDraft}
          onRun={() => run(draft)}
          running={results.isFetching}
          error={parseError}
        />

        <SavedRail
          queries={saved.data ?? []}
          activeJql={active}
          onPick={(q) => {
            setDraft(q.jql);
            run(q.jql);
          }}
          onSaveClick={() => setSaveOpen(true)}
          onChanged={() => void qc.invalidateQueries({ queryKey: ["saved-queries"] })}
        />

        {saveOpen && (
          <SaveForm
            jql={draft.trim()}
            onClose={() => setSaveOpen(false)}
            onSaved={() => {
              setSaveOpen(false);
              void qc.invalidateQueries({ queryKey: ["saved-queries"] });
            }}
          />
        )}

        {error && !parseError ? (
          <LoadError
            what="The search"
            message={error.message}
            onRetry={() => void results.refetch()}
          />
        ) : (
          <section className="space-y-3">
            <div className="mono flex flex-wrap items-center gap-2 text-xs text-chrome-dim">
              <span data-testid="jql-count">
                {results.isLoading
                  ? "counting…"
                  : total === 0
                    ? "no matches"
                    : `${from}–${to} of ${total}`}
              </span>
              {total > PAGE && (
                <span className="ml-auto flex items-center gap-1">
                  <button
                    type="button"
                    aria-label="previous page"
                    disabled={offset === 0}
                    onClick={() => setOffset((o) => Math.max(0, o - PAGE))}
                    className="rounded border border-white/10 p-1 hover:border-white/20 disabled:opacity-30"
                  >
                    <ChevronLeft size={12} />
                  </button>
                  <button
                    type="button"
                    aria-label="next page"
                    disabled={to >= total}
                    onClick={() => setOffset((o) => o + PAGE)}
                    className="rounded border border-white/10 p-1 hover:border-white/20 disabled:opacity-30"
                  >
                    <ChevronRight size={12} />
                  </button>
                </span>
              )}
            </div>
            <ResultTable items={items} empty={!results.isLoading && total === 0} />
          </section>
        )}

        <Cheatsheet
          onExample={(jql) => {
            setDraft(jql);
            run(jql);
          }}
        />
      </div>
    </AppShell>
  );
}

function QueryEditor({
  value,
  onChange,
  onRun,
  running,
  error,
}: {
  value: string;
  onChange: (v: string) => void;
  onRun: () => void;
  running: boolean;
  error: string | null;
}) {
  return (
    <section className="space-y-2">
      <label htmlFor="jql" className="mono block text-xs text-chrome-dim">
        query
      </label>
      <div className="flex flex-wrap items-start gap-2">
        <textarea
          id="jql"
          aria-label="query"
          rows={2}
          spellCheck={false}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            // ⌘/Ctrl+Enter runs; plain Enter would fight multi-line queries.
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              onRun();
            }
          }}
          placeholder="assignee = currentUser() AND status != done ORDER BY priority ASC"
          className="mono min-w-0 flex-1 resize-y rounded border border-white/10 bg-black/30 p-2.5 text-sm text-chrome outline-none placeholder:text-chrome-dim/60 focus:border-accent/50"
        />
        <button
          type="button"
          onClick={onRun}
          disabled={running}
          className="mono inline-flex items-center gap-1.5 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
        >
          <Play size={12} /> {running ? "running…" : "$ run"}
        </button>
      </div>
      <p className="mono text-[11px] text-chrome-dim">⌘↵ to run</p>
      {error && (
        <p
          role="alert"
          data-testid="jql-error"
          className="mono rounded border border-red-500/30 bg-red-500/10 p-2.5 text-xs text-red-200"
        >
          {error}
        </p>
      )}
    </section>
  );
}

function SavedRail({
  queries,
  activeJql,
  onPick,
  onSaveClick,
  onChanged,
}: {
  queries: SavedQuery[];
  activeJql: string;
  onPick: (q: SavedQuery) => void;
  onSaveClick: () => void;
  onChanged: () => void;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState("");

  const rename = useMutation({
    mutationFn: (v: { id: string; name: string }) =>
      updateSavedQuery(v.id, { name: v.name }),
    onSuccess: () => {
      setRenaming(null);
      onChanged();
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteSavedQuery(id),
    onSuccess: onChanged,
  });
  const failure = (rename.error ?? remove.error) as unknown as ApiError | undefined;

  return (
    <section className="space-y-2" aria-label="saved queries">
      <div className="mono flex items-center gap-2 text-xs text-chrome-dim">
        <span>saved</span>
        <button
          type="button"
          onClick={onSaveClick}
          className="ml-auto inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 hover:border-white/20 hover:text-chrome"
        >
          <BookmarkPlus size={11} /> save this query
        </button>
      </div>
      {queries.length === 0 ? (
        <p className="mono text-xs text-chrome-dim/70">
          Nothing saved yet. A query you keep retyping is a query worth naming.
        </p>
      ) : (
        <ul className="flex flex-wrap gap-2">
          {queries.map((q) => (
            <li key={q.id}>
              <span
                className={`mono inline-flex items-center gap-1.5 rounded border px-2 py-1 text-xs ${
                  q.jql === activeJql
                    ? "border-accent/50 bg-accent/10 text-chrome"
                    : "border-white/10 text-chrome-dim"
                }`}
              >
                {renaming === q.id ? (
                  <>
                    <input
                      aria-label="new name"
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      className="mono w-36 rounded border border-white/10 bg-black/30 px-1.5 py-0.5 text-xs text-chrome outline-none focus:border-accent/50"
                    />
                    <button
                      type="button"
                      onClick={() => rename.mutate({ id: q.id, name })}
                      className="text-accent hover:underline"
                    >
                      save
                    </button>
                    <button
                      type="button"
                      onClick={() => setRenaming(null)}
                      className="text-chrome-dim hover:text-chrome"
                    >
                      cancel
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      onClick={() => onPick(q)}
                      title={q.jql}
                      className="hover:text-chrome"
                    >
                      {q.name}
                    </button>
                    {q.is_shared && (
                      <Users
                        size={10}
                        className="text-chrome-dim"
                        aria-label={
                          q.is_mine ? "shared with everyone" : `shared by @${q.owner_handle}`
                        }
                      />
                    )}
                    {q.is_mine && (
                      <>
                        <button
                          type="button"
                          aria-label={`rename ${q.name}`}
                          onClick={() => {
                            setRenaming(q.id);
                            setName(q.name);
                          }}
                          className="text-chrome-dim hover:text-chrome"
                        >
                          <Pencil size={10} />
                        </button>
                        <button
                          type="button"
                          aria-label={`delete ${q.name}`}
                          onClick={() => remove.mutate(q.id)}
                          className="text-chrome-dim hover:text-red-300"
                        >
                          <Trash2 size={10} />
                        </button>
                      </>
                    )}
                  </>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
      {failure && (
        <p role="alert" className="mono text-xs text-red-300">
          {failure.message}
        </p>
      )}
    </section>
  );
}

function SaveForm({
  jql,
  onClose,
  onSaved,
}: {
  jql: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState("");
  const [shared, setShared] = useState(false);
  const save = useMutation({
    mutationFn: () => createSavedQuery({ name, jql, is_shared: shared }),
    onSuccess: onSaved,
  });
  const failure = save.error as unknown as ApiError | undefined;

  return (
    <form
      aria-label="save query"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate();
      }}
      className="space-y-3 rounded border border-white/10 bg-black/20 p-3"
    >
      <div className="flex flex-wrap items-end gap-3">
        <div className="min-w-0 flex-1">
          <label htmlFor="qname" className="mono block text-xs text-chrome-dim">
            name
          </label>
          <input
            id="qname"
            aria-label="query name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my open work"
            className="mono mt-1 w-full rounded border border-white/10 bg-black/30 px-2 py-1.5 text-sm text-chrome outline-none focus:border-accent/50"
          />
        </div>
        <label className="mono flex items-center gap-1.5 text-xs text-chrome-dim">
          <input
            type="checkbox"
            checked={shared}
            onChange={(e) => setShared(e.target.checked)}
            className="accent-accent"
          />
          share with everyone
        </label>
        <button
          type="submit"
          disabled={!name.trim() || !jql || save.isPending}
          className="mono rounded bg-accent px-3 py-1.5 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
        >
          {save.isPending ? "saving…" : "save"}
        </button>
        <button
          type="button"
          onClick={onClose}
          className="mono text-xs text-chrome-dim hover:text-chrome"
        >
          cancel
        </button>
      </div>
      <p className="mono truncate text-[11px] text-chrome-dim">
        {jql ? jql : "Write a query first — there's nothing to save yet."}
      </p>
      {failure && (
        <p role="alert" className="mono text-xs text-red-300">
          {failure.message}
        </p>
      )}
    </form>
  );
}

function ResultTable({ items, empty }: { items: JqlHit[]; empty: boolean }) {
  if (empty) {
    return (
      <p className="mono rounded border border-white/10 p-4 text-xs text-chrome-dim">
        Nothing matched. Loosen a condition, or check a value against the field list below.
      </p>
    );
  }
  return (
    <div className="overflow-x-auto rounded border border-white/10">
      <table className="mono w-full min-w-[720px] text-left text-xs">
        <thead className="text-chrome-dim">
          <tr className="border-b border-white/10">
            <th className="px-3 py-2 font-normal">key</th>
            <th className="px-3 py-2 font-normal">title</th>
            <th className="px-3 py-2 font-normal">status</th>
            <th className="px-3 py-2 font-normal">pri</th>
            <th className="px-3 py-2 font-normal">assignee</th>
            <th className="px-3 py-2 font-normal">sprint</th>
            <th className="px-3 py-2 font-normal">pts</th>
            <th className="px-3 py-2 font-normal">due</th>
          </tr>
        </thead>
        <tbody data-testid="jql-results">
          {items.map((t) => (
            <tr key={t.key} className="border-b border-white/5 last:border-0 hover:bg-white/5">
              <td className="whitespace-nowrap px-3 py-2">
                <Link href={`/tasks/${t.key}`} className="text-accent hover:underline">
                  {t.key}
                </Link>
              </td>
              <td className="max-w-[24rem] truncate px-3 py-2 text-chrome" title={t.title}>
                {t.title}
              </td>
              <td className="whitespace-nowrap px-3 py-2 text-chrome-dim">{t.status}</td>
              <td className="whitespace-nowrap px-3 py-2">
                <span style={{ color: PRIORITY_COLOR[t.priority] ?? undefined }}>
                  {t.priority}
                </span>
              </td>
              <td className="whitespace-nowrap px-3 py-2 text-chrome-dim">
                {t.assignee_handle ? `@${t.assignee_handle}` : "—"}
              </td>
              <td className="max-w-[10rem] truncate px-3 py-2 text-chrome-dim">
                {t.sprint_name ?? "backlog"}
              </td>
              <td className="whitespace-nowrap px-3 py-2 text-chrome-dim">
                {t.story_points ?? "—"}
              </td>
              <td className="whitespace-nowrap px-3 py-2 text-chrome-dim">
                {t.due_date ?? "—"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Cheatsheet({ onExample }: { onExample: (jql: string) => void }) {
  const fields = useMemo(() => JQL_FIELDS.join(" · "), []);
  return (
    <details className="rounded border border-white/10 p-3">
      <summary className="mono cursor-pointer text-xs text-chrome-dim">
        what can I write?
      </summary>
      <div className="mono mt-3 space-y-3 text-xs text-chrome-dim">
        <div>
          <div className="text-chrome">fields</div>
          <p className="mt-1 leading-relaxed">{fields}</p>
        </div>
        <div>
          <div className="text-chrome">operators</div>
          <p className="mt-1 leading-relaxed">
            {"= != ~ (contains) !~ > >= < <= · in (a, b) · not in (a, b) · is empty · is not empty"}
          </p>
        </div>
        <div>
          <div className="text-chrome">joining, negating, ordering</div>
          <p className="mt-1 leading-relaxed">
            {"AND · OR (AND binds tighter) · NOT · parentheses · ORDER BY field ASC|DESC, …"}
          </p>
        </div>
        <div>
          <div className="text-chrome">values</div>
          <p className="mt-1 leading-relaxed">
            Quote anything with spaces. <span className="text-chrome">currentUser()</span> is you.
            Dates take <span className="text-chrome">2026-08-04</span>,{" "}
            <span className="text-chrome">today</span>, or an offset like{" "}
            <span className="text-chrome">-7d</span> / <span className="text-chrome">2w</span>.
          </p>
        </div>
        <div>
          <div className="text-chrome">start from one of these</div>
          <ul className="mt-1 flex flex-wrap gap-2">
            {JQL_EXAMPLES.map((e) => (
              <li key={e.label}>
                <button
                  type="button"
                  onClick={() => onExample(e.jql)}
                  title={e.jql}
                  className="rounded border border-white/10 px-2 py-1 hover:border-white/20 hover:text-chrome"
                >
                  {e.label}
                </button>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </details>
  );
}
