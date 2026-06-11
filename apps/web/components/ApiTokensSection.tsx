"use client";

// Settings section: personal API tokens. Create shows the secret once;
// after that it's hashes all the way down. Revoke is immediate.

import { useEffect, useState } from "react";
import { Check, Copy, Trash2 } from "lucide-react";
import {
  createApiToken,
  listApiTokens,
  revokeApiToken,
  type ApiToken,
  type ApiTokenScope,
} from "@/lib/apiTokens";
import type { ApiError } from "@/lib/api";

export function ApiTokensSection() {
  const [tokens, setTokens] = useState<ApiToken[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [write, setWrite] = useState(false);
  const [creating, setCreating] = useState(false);
  const [freshSecret, setFreshSecret] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  async function reload() {
    try {
      setTokens(await listApiTokens());
    } catch (e) {
      setError((e as unknown as ApiError).message);
    }
  }
  useEffect(() => {
    void reload();
  }, []);

  async function create(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setCreating(true);
    setError(null);
    try {
      const scopes: ApiTokenScope[] = write ? ["read", "write"] : ["read"];
      const res = await createApiToken({ name: name.trim(), scopes });
      setFreshSecret(res.secret);
      setCopied(false);
      setName("");
      setWrite(false);
      await reload();
    } catch (e) {
      setError((e as unknown as ApiError).message);
    } finally {
      setCreating(false);
    }
  }

  return (
    <section className="space-y-3 border-t border-white/10 pt-6">
      <div>
        <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
          API tokens
        </h2>
        <p className="mt-1 text-xs text-chrome-dim">
          For scripts and CI:{" "}
          <span className="mono">Authorization: Bearer slt_…</span>. Read-only
          unless you say otherwise. Revoking takes effect on the next request.
        </p>
      </div>

      {freshSecret && (
        <div className="space-y-1 rounded border border-accent/40 bg-accent/10 p-3">
          <div className="mono text-[11px] uppercase tracking-widest text-chrome-dim">
            copy it now — this is the only time we show it
          </div>
          <div className="flex items-center gap-2">
            <code className="mono flex-1 break-all text-xs text-chrome">{freshSecret}</code>
            <button
              type="button"
              aria-label="copy token"
              onClick={async () => {
                await navigator.clipboard.writeText(freshSecret);
                setCopied(true);
              }}
              className="text-chrome-dim hover:text-chrome"
            >
              {copied ? <Check size={14} /> : <Copy size={14} />}
            </button>
          </div>
        </div>
      )}

      <ul className="space-y-1">
        {(tokens ?? []).map((t) => (
          <li
            key={t.id}
            className="flex items-center gap-2 rounded border border-white/10 px-2 py-1.5"
          >
            <span className="mono text-xs text-chrome">{t.name}</span>
            <span className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-chrome-dim">
              {t.scopes.includes("write") ? "read+write" : "read-only"}
            </span>
            <span className="mono ml-auto text-[10px] text-chrome-dim">
              {t.last_used_at
                ? `used ${new Date(t.last_used_at).toISOString().slice(0, 10)}`
                : "never used"}
            </span>
            <button
              type="button"
              aria-label={`revoke ${t.name}`}
              onClick={() => {
                if (confirm(`Revoke "${t.name}"? Scripts using it start failing immediately.`))
                  revokeApiToken(t.id).then(reload, (e) =>
                    setError((e as ApiError).message),
                  );
              }}
              className="text-chrome-dim hover:text-red-300"
            >
              <Trash2 size={13} />
            </button>
          </li>
        ))}
        {tokens?.length === 0 && (
          <li className="mono text-[11px] text-chrome-dim">
            no tokens — your scripts are still doing it by hand
          </li>
        )}
      </ul>

      <form onSubmit={create} className="flex items-center gap-2">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          maxLength={60}
          placeholder="token name (e.g. ci-deploy)"
          className="mono flex-1 rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
        />
        <label className="mono flex items-center gap-1 text-[11px] text-chrome-dim">
          <input
            type="checkbox"
            checked={write}
            onChange={(e) => setWrite(e.target.checked)}
          />
          write
        </label>
        <button
          type="submit"
          disabled={creating || !name.trim()}
          className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
        >
          mint
        </button>
      </form>

      {error && <div className="mono text-[11px] text-red-300">{error}</div>}
    </section>
  );
}
