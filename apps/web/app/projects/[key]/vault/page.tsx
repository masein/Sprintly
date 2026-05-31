"use client";

// /projects/[key]/vault — per-project secrets store.
//
// On route navigation away from this page, React unmounts the tree → every
// row's local `revealed` state goes out of scope. We rely on that for the
// "wipe on route change" invariant (the api wrapper never caches reveal
// responses, and revealVaultItem isn't a useQuery).

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams, useRouter } from "next/navigation";
import { Plus, X, Vault as VaultIcon } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { VaultItemRow } from "@/components/VaultItemRow";
import {
  createVaultItem,
  listVaultItems,
  listVaultAccess,
  listVaultAudit,
  grantVaultAccess,
  revokeVaultAccess,
  type VaultKind,
} from "@/lib/vault";
import { search } from "@/lib/search";
import { getProject } from "@/lib/projects";
import type { ApiError } from "@/lib/api";

const KINDS: VaultKind[] = ["password", "api_key", "ssh_key", "note", "env_file"];

export default function VaultPage() {
  const router = useRouter();
  const params = useParams<{ key: string }>();
  const projectKey = params?.key ?? "";

  const projectQ = useQuery({
    queryKey: ["project", projectKey],
    queryFn: () => getProject(projectKey),
    enabled: !!projectKey,
  });
  const itemsQ = useQuery({
    queryKey: ["vault", projectKey],
    queryFn: () => listVaultItems(projectKey),
    enabled: !!projectKey,
  });

  const canEdit =
    projectQ.data?.your_role === "lead" || projectQ.data?.your_role === "contributor";
  // Only leads (and admins; the API enforces it) can create/manage access.
  const canManage = projectQ.data?.your_role === "lead";

  const [creating, setCreating] = useState(false);
  const [auditId, setAuditId] = useState<string | null>(null);
  const [accessId, setAccessId] = useState<string | null>(null);

  if (itemsQ.error) {
    const e = itemsQ.error as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
  }

  const grouped = (itemsQ.data ?? []).reduce<Record<string, typeof itemsQ.data>>(
    (acc, it) => {
      const k = it.kind;
      (acc[k] ||= []).push(it);
      return acc;
    },
    {} as Record<string, typeof itemsQ.data>,
  );

  return (
    <AppShell currentProjectKey={projectKey}>
      <header className="mb-6 flex items-end justify-between">
        <div>
          <div className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
            <VaultIcon size={11} /> sprintly · {projectKey} · vault
          </div>
          <h1 className="text-3xl font-semibold">Secrets.</h1>
          <p className="mt-1 text-sm text-chrome-dim">
            Encrypted at rest with a per-project key. Reveal is rate-limited
            and audit-logged.
          </p>
        </div>
        {canManage && (
          <button
            type="button"
            onClick={() => setCreating(true)}
            className="mono inline-flex items-center gap-1 rounded bg-accent px-3 py-1.5 text-sm font-medium text-accent-fg hover:opacity-90"
          >
            <Plus size={14} /> add
          </button>
        )}
      </header>

      {creating && (
        <CreateVaultForm
          projectKey={projectKey}
          onClose={() => setCreating(false)}
        />
      )}

      {itemsQ.isLoading && (
        <div className="mono text-sm text-chrome-dim">compiling vibes…</div>
      )}

      {itemsQ.data && itemsQ.data.length === 0 && !creating && (
        <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
          <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
            empty vault
          </div>
          <p className="text-chrome-dim">
            No secrets yet. Treat this like 1Password for the project — DB
            creds, deploy keys, .env files.
          </p>
        </div>
      )}

      <div className="space-y-6">
        {KINDS.map((kind) => {
          const items = grouped[kind] ?? [];
          if (items.length === 0) return null;
          return (
            <section key={kind}>
              <h2 className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
                {kind.replace("_", " ")} ({items.length})
              </h2>
              <ul className="space-y-2">
                {items.map((it) => (
                  <VaultItemRow
                    key={it.id}
                    item={it}
                    canEdit={canEdit}
                    projectKey={projectKey}
                    onShowAudit={(id) => setAuditId(id)}
                    onShowAccess={(id) => setAccessId(id)}
                  />
                ))}
              </ul>
            </section>
          );
        })}
      </div>

      {auditId && <AuditDrawer id={auditId} onClose={() => setAuditId(null)} />}
      {accessId && (
        <AccessDrawer id={accessId} onClose={() => setAccessId(null)} />
      )}
    </AppShell>
  );
}

function CreateVaultForm({
  projectKey,
  onClose,
}: {
  projectKey: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<VaultKind>("password");
  const [description, setDescription] = useState("");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: () =>
      createVaultItem(projectKey, {
        name,
        kind,
        description: description || undefined,
        value,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["vault", projectKey] });
      onClose();
    },
    onError: (e) => setError((e as ApiError).message),
  });

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim() && value) create.mutate();
      }}
      className="mb-6 space-y-3 rounded-lg border border-white/10 bg-ink-subtle p-4"
    >
      <div className="flex items-center justify-between">
        <span className="mono text-xs uppercase tracking-widest text-chrome-dim">
          new vault item
        </span>
        <button
          type="button"
          onClick={onClose}
          className="text-chrome-dim hover:text-chrome"
          aria-label="Close"
        >
          <X size={14} />
        </button>
      </div>
      <div className="flex items-center gap-2">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="name (e.g. prod DB password)"
          required
          className="flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
        />
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value as VaultKind)}
          className="mono rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome"
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>{k}</option>
          ))}
        </select>
      </div>
      <input
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        placeholder="description (optional, NEVER put the secret here)"
        className="block w-full rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <textarea
        value={value}
        onChange={(e) => setValue(e.target.value)}
        required
        rows={kind === "env_file" || kind === "ssh_key" ? 6 : 2}
        placeholder="the actual secret value"
        className="mono block w-full rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      {error && (
        <div className="mono rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
          {error}
        </div>
      )}
      <div className="flex items-center justify-between">
        <span className="mono text-[10px] text-chrome-dim">
          encrypts client-side after submit (never logged)
        </span>
        <button
          type="submit"
          disabled={!name.trim() || !value || create.isPending}
          className="mono rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50"
        >
          {create.isPending ? "encrypting…" : "$ git add secret"}
        </button>
      </div>
    </form>
  );
}

function AuditDrawer({ id, onClose }: { id: string; onClose: () => void }) {
  const q = useQuery({
    queryKey: ["vault-audit", id],
    queryFn: () => listVaultAudit(id),
  });
  return (
    <Drawer onClose={onClose} title="audit log">
      {q.isLoading && (
        <div className="mono text-xs text-chrome-dim">compiling…</div>
      )}
      <ul className="space-y-1">
        {(q.data ?? []).map((r) => (
          <li
            key={r.id}
            className="mono flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2 py-1.5 text-xs"
          >
            <span className="text-chrome">{r.action}</span>
            <span className="text-chrome-dim">@{r.user_handle ?? "?"}</span>
            <span className="ml-auto text-[10px] text-chrome-dim">
              {new Date(r.occurred_at).toISOString().slice(0, 19).replace("T", " ")}
            </span>
          </li>
        ))}
      </ul>
    </Drawer>
  );
}

function AccessDrawer({ id, onClose }: { id: string; onClose: () => void }) {
  const qc = useQueryClient();
  const q = useQuery({
    queryKey: ["vault-access", id],
    queryFn: () => listVaultAccess(id),
  });
  const [searchTerm, setSearchTerm] = useState("");
  const userHits = useQuery({
    queryKey: ["vault-access-search", searchTerm],
    queryFn: () => search(searchTerm, 5),
    enabled: searchTerm.length >= 2,
  });
  const grant = useMutation({
    mutationFn: ({ user_id, can_edit }: { user_id: string; can_edit: boolean }) =>
      grantVaultAccess(id, { user_id, can_view: true, can_edit }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["vault-access", id] }),
  });
  const revoke = useMutation({
    mutationFn: (user_id: string) => revokeVaultAccess(id, user_id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["vault-access", id] }),
  });

  return (
    <Drawer onClose={onClose} title="manage access">
      <ul className="mb-3 space-y-1">
        {(q.data ?? []).map((row) => (
          <li
            key={row.user_id}
            className="mono flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-2 py-1.5 text-xs"
          >
            <span className="text-chrome">@{row.handle}</span>
            <span className="ml-auto text-[10px] uppercase text-chrome-dim">
              {row.can_edit ? "view+edit" : "view"}
            </span>
            <button
              type="button"
              onClick={() => revoke.mutate(row.user_id)}
              className="text-chrome-dim hover:text-red-300"
              aria-label="revoke"
            >
              ×
            </button>
          </li>
        ))}
        {q.data?.length === 0 && (
          <li className="mono text-[11px] text-chrome-dim">
            only project leads + admins have access
          </li>
        )}
      </ul>
      <div className="border-t border-white/10 pt-3">
        <input
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          placeholder="search users to grant…"
          className="block w-full rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
        />
        <ul className="mt-2 space-y-1">
          {(userHits.data?.users ?? []).map((u) => (
            <li
              key={u.id}
              className="mono flex items-center gap-2 rounded px-1 py-1 text-xs"
            >
              <span className="text-chrome">@{u.handle}</span>
              <span className="ml-auto flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => grant.mutate({ user_id: u.id, can_edit: false })}
                  className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-chrome-dim hover:border-white/20 hover:text-chrome"
                >
                  view
                </button>
                <button
                  type="button"
                  onClick={() => grant.mutate({ user_id: u.id, can_edit: true })}
                  className="mono rounded border border-white/10 px-1.5 py-0.5 text-[10px] text-chrome-dim hover:border-white/20 hover:text-chrome"
                >
                  view+edit
                </button>
              </span>
            </li>
          ))}
        </ul>
      </div>
    </Drawer>
  );
}

function Drawer({
  onClose,
  title,
  children,
}: {
  onClose: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-stretch justify-end bg-black/60"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <aside className="w-full max-w-md overflow-y-auto border-l border-white/10 bg-ink p-5">
        <header className="mb-4 flex items-center justify-between">
          <h2 className="mono text-xs uppercase tracking-widest text-chrome-dim">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-chrome-dim hover:text-chrome"
            aria-label="Close"
          >
            <X size={14} />
          </button>
        </header>
        {children}
      </aside>
    </div>
  );
}
