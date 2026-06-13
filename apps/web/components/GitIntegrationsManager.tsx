"use client";

// Connect a repo to the project: provider + repo + optional self-hosted URL
// and API token. On create we show the webhook URL + secret exactly once —
// paste both into the provider's webhook settings and you're wired up.

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, Trash2, X } from "lucide-react";
import {
  createGitIntegration,
  deleteGitIntegration,
  listGitIntegrations,
  updateGitIntegration,
  type GitProvider,
} from "@/lib/gitIntegrations";
import type { ApiError } from "@/lib/api";

const PROVIDERS: GitProvider[] = ["github", "gitlab", "gitea"];

export function GitIntegrationsManager({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["git-integrations", projectKey],
    queryFn: () => listGitIntegrations(projectKey),
    retry: false,
  });
  const invalidate = () =>
    qc.invalidateQueries({ queryKey: ["git-integrations", projectKey] });

  const [provider, setProvider] = useState<GitProvider>("github");
  const [repo, setRepo] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiToken, setApiToken] = useState("");
  const [statusEnabled, setStatusEnabled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fresh, setFresh] = useState<{ url: string; secret: string } | null>(null);
  const [copied, setCopied] = useState<"url" | "secret" | null>(null);

  const add = useMutation({
    mutationFn: () =>
      createGitIntegration(projectKey, {
        provider,
        repo: repo.trim(),
        base_url: baseUrl.trim() || undefined,
        api_token: apiToken.trim() || undefined,
        status_enabled: statusEnabled,
      }),
    onSuccess: (res) => {
      setFresh({
        url: `${window.location.origin}${res.webhook_path}`,
        secret: res.webhook_secret,
      });
      setCopied(null);
      setRepo("");
      setBaseUrl("");
      setApiToken("");
      setStatusEnabled(false);
      setError(null);
      invalidate();
    },
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const toggleStatus = useMutation({
    mutationFn: (v: { id: string; enabled: boolean }) =>
      updateGitIntegration(v.id, { status_enabled: v.enabled }),
    onSuccess: invalidate,
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteGitIntegration(id),
    onSuccess: invalidate,
    onError: (e) => setError((e as unknown as ApiError).message ?? "failed"),
  });

  const items = q.data ?? [];

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-lg space-y-4 rounded-lg border border-white/10 bg-ink-subtle p-6">
        <div className="flex items-start justify-between">
          <div>
            <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
              {projectKey} · git
            </div>
            <h2 className="text-xl font-semibold">Connected repos</h2>
            <p className="mt-1 text-xs text-chrome-dim">
              Branches, commits and PRs that mention a task key (like{" "}
              <span className="mono">{projectKey}-1</span>) link themselves to
              the task. Merges move it to done.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        {fresh && (
          <div className="space-y-2 rounded border border-accent/40 bg-accent/10 p-3">
            <div className="mono text-[11px] uppercase tracking-widest text-chrome-dim">
              webhook config — the secret shows once, copy both now
            </div>
            <SecretRow
              label="payload url"
              value={fresh.url}
              copied={copied === "url"}
              onCopy={async () => {
                await navigator.clipboard.writeText(fresh.url);
                setCopied("url");
              }}
            />
            <SecretRow
              label="secret"
              value={fresh.secret}
              copied={copied === "secret"}
              onCopy={async () => {
                await navigator.clipboard.writeText(fresh.secret);
                setCopied("secret");
              }}
            />
            <p className="mono text-[10px] text-chrome-dim">
              provider settings → webhooks → content type application/json,
              events: pushes + pull/merge requests
            </p>
          </div>
        )}

        <ul className="space-y-1">
          {items.map((gi) => (
            <li
              key={gi.id}
              className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5"
            >
              <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim">
                {gi.provider}
              </span>
              <span className="mono truncate text-xs text-chrome">{gi.repo}</span>
              <label className="mono ml-auto flex shrink-0 items-center gap-1 text-[10px] text-chrome-dim">
                <input
                  type="checkbox"
                  checked={gi.status_enabled}
                  disabled={!gi.has_api_token}
                  title={
                    gi.has_api_token
                      ? "push task state to commit statuses"
                      : "needs an API token"
                  }
                  onChange={(e) =>
                    toggleStatus.mutate({ id: gi.id, enabled: e.target.checked })
                  }
                />
                status
              </label>
              <button
                type="button"
                aria-label={`disconnect ${gi.repo}`}
                onClick={() => {
                  if (
                    confirm(
                      `Disconnect ${gi.repo}? Existing task links stay; new events stop.`,
                    )
                  )
                    remove.mutate(gi.id);
                }}
                className="shrink-0 text-chrome-dim hover:text-red-300"
              >
                <Trash2 size={13} />
              </button>
            </li>
          ))}
          {items.length === 0 && (
            <li className="mono text-[11px] text-chrome-dim">
              no repos connected — the board and the code are strangers
            </li>
          )}
        </ul>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (repo.trim()) add.mutate();
          }}
          className="space-y-2 border-t border-white/10 pt-3"
        >
          <div className="flex items-center gap-2">
            <select
              value={provider}
              onChange={(e) => setProvider(e.target.value as GitProvider)}
              aria-label="provider"
              className="mono rounded border border-white/10 bg-ink px-1.5 py-1 text-xs text-chrome"
            >
              {PROVIDERS.map((p) => (
                <option key={p} value={p}>{p}</option>
              ))}
            </select>
            <input
              value={repo}
              onChange={(e) => setRepo(e.target.value)}
              placeholder="owner/repo"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <button
              type="submit"
              disabled={add.isPending || !repo.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              connect
            </button>
          </div>
          {(provider === "gitea" || provider === "gitlab") && (
            <input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder={
                provider === "gitea"
                  ? "base url (required), e.g. https://git.acme.dev"
                  : "base url — empty for gitlab.com"
              }
              className="mono w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
          )}
          <div className="flex items-center gap-2">
            <input
              value={apiToken}
              onChange={(e) => setApiToken(e.target.value)}
              type="password"
              placeholder="API token (optional — enables commit statuses)"
              className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <label className="mono flex items-center gap-1 text-[11px] text-chrome-dim">
              <input
                type="checkbox"
                checked={statusEnabled}
                disabled={!apiToken.trim()}
                onChange={(e) => setStatusEnabled(e.target.checked)}
              />
              status
            </label>
          </div>
          {error && <div className="mono text-[11px] text-red-300">{error}</div>}
        </form>
      </div>
    </div>
  );
}

function SecretRow({
  label,
  value,
  copied,
  onCopy,
}: {
  label: string;
  value: string;
  copied: boolean;
  onCopy: () => void;
}) {
  return (
    <div>
      <div className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
        {label}
      </div>
      <div className="flex items-center gap-2">
        <code className="mono flex-1 break-all text-xs text-chrome">{value}</code>
        <button
          type="button"
          aria-label={`copy ${label}`}
          onClick={onCopy}
          className="text-chrome-dim hover:text-chrome"
        >
          {copied ? <Check size={14} /> : <Copy size={14} />}
        </button>
      </div>
    </div>
  );
}
