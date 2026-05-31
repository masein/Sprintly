"use client";

// /admin — one page with five tabs. Mainly bookkeeping: users, audit, health,
// backups, webhooks-scaffolding.

import { useState } from "react";
import { useRouter } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity, AlertTriangle, Check, Cog, Database, Download, RotateCcw,
  Server, Shield, Users, Webhook,
} from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { StatTile } from "@/components/StatTile";
import {
  getHealth,
  listAdminAudit,
  listAdminUsers,
  listBackups,
  reactivateUser,
  resetUserPassword,
  setUserRole,
  startBackup,
  suspendUser,
  type AdminUserRow,
} from "@/lib/admin";
import type { ApiError } from "@/lib/api";

type Tab = "users" | "audit" | "health" | "backups" | "webhooks";

export default function AdminPage() {
  const router = useRouter();
  const [tab, setTab] = useState<Tab>("users");

  return (
    <AppShell>
      <header className="mb-6">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · admin
        </div>
        <h1 className="text-3xl font-semibold">Operations.</h1>
        <p className="mt-1 text-sm text-chrome-dim">
          Admin-only. Every action you take here is in the audit log.
        </p>
      </header>

      <nav className="mb-6 flex gap-1 border-b border-white/10">
        <TabButton current={tab} self="users"    onClick={() => setTab("users")}    icon={Users}>users</TabButton>
        <TabButton current={tab} self="audit"    onClick={() => setTab("audit")}    icon={Shield}>audit</TabButton>
        <TabButton current={tab} self="health"   onClick={() => setTab("health")}   icon={Activity}>health</TabButton>
        <TabButton current={tab} self="backups"  onClick={() => setTab("backups")}  icon={Database}>backups</TabButton>
        <TabButton current={tab} self="webhooks" onClick={() => setTab("webhooks")} icon={Webhook}>webhooks</TabButton>
      </nav>

      {tab === "users" && <UsersTab onAuthExpired={() => router.push("/login")} />}
      {tab === "audit" && <AuditTab />}
      {tab === "health" && <HealthTab />}
      {tab === "backups" && <BackupsTab />}
      {tab === "webhooks" && <WebhooksTab />}
    </AppShell>
  );
}

function TabButton({
  current, self, onClick, icon: Icon, children,
}: {
  current: Tab; self: Tab; onClick: () => void;
  icon: React.ComponentType<{ size?: string | number }>;
  children: React.ReactNode;
}) {
  const active = current === self;
  return (
    <button
      type="button"
      onClick={onClick}
      className={`mono -mb-px inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs uppercase tracking-widest ${
        active
          ? "border-accent text-chrome"
          : "border-transparent text-chrome-dim hover:text-chrome"
      }`}
    >
      <Icon size={12} /> {children}
    </button>
  );
}

// ─── Users ──────────────────────────────────────────────────────────────────

function UsersTab({ onAuthExpired }: { onAuthExpired: () => void }) {
  const qc = useQueryClient();
  const [q, setQ] = useState("");
  const [status, setStatus] = useState<string>("");
  const users = useQuery({
    queryKey: ["admin-users", q, status],
    queryFn: () => listAdminUsers({ q: q || undefined, status: status || undefined }),
    retry: (n, e) => {
      const ae = e as unknown as ApiError;
      if (ae?.status === 401) {
        onAuthExpired();
        return false;
      }
      return ae?.status !== 403 && n < 1;
    },
  });

  const susp = useMutation({
    mutationFn: (id: string) => suspendUser(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin-users"] }),
  });
  const react_ = useMutation({
    mutationFn: (id: string) => reactivateUser(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin-users"] }),
  });
  const role = useMutation({
    mutationFn: ({ id, role: r }: { id: string; role: AdminUserRow["role"] }) =>
      setUserRole(id, r),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin-users"] }),
  });
  const reset = useMutation({
    mutationFn: (id: string) => resetUserPassword(id),
    onSuccess: (data) => {
      navigator.clipboard.writeText(data.url).catch(() => {});
      alert(`Reset URL copied to clipboard. Expires ${data.expires_at}.`);
    },
  });

  if (users.error) {
    const ae = users.error as unknown as ApiError;
    if (ae.status === 403) {
      return (
        <div className="mono rounded border border-white/10 bg-ink-subtle p-6 text-sm text-chrome-dim">
          Admin-only. Ask another admin to flip your role.
        </div>
      );
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="search handle / email / name"
          className="mono w-72 rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
        />
        <select
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="mono rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome"
        >
          <option value="">all status</option>
          <option value="active">active</option>
          <option value="invited">invited</option>
          <option value="suspended">suspended</option>
        </select>
      </div>

      <ul className="space-y-1">
        {(users.data ?? []).map((u) => (
          <li
            key={u.id}
            className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2"
          >
            <div className="min-w-0 flex-1">
              <div className="mono text-sm text-chrome">@{u.handle}</div>
              <div className="mono text-[11px] text-chrome-dim">
                {u.email} · {u.display_name}
              </div>
            </div>
            <span
              className={`mono rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${
                u.status === "active"
                  ? "border-emerald-500/30 text-emerald-300"
                  : u.status === "suspended"
                    ? "border-red-500/30 text-red-300"
                    : "border-white/10 text-chrome-dim"
              }`}
            >
              {u.status}
            </span>
            <select
              value={u.role}
              onChange={(e) =>
                role.mutate({ id: u.id, role: e.target.value as AdminUserRow["role"] })
              }
              className="mono rounded border border-white/10 bg-ink px-1.5 py-0.5 text-[11px] text-chrome"
            >
              <option value="admin">admin</option>
              <option value="member">member</option>
              <option value="viewer">viewer</option>
            </select>
            {u.status === "suspended" ? (
              <button
                type="button"
                onClick={() => react_.mutate(u.id)}
                className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
              >
                <RotateCcw size={10} /> reactivate
              </button>
            ) : (
              <button
                type="button"
                onClick={() => {
                  if (confirm(`Suspend @${u.handle}? They lose all sessions immediately.`)) {
                    susp.mutate(u.id);
                  }
                }}
                className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-[11px] text-chrome-dim hover:border-red-500/40 hover:text-red-300"
              >
                <AlertTriangle size={10} /> suspend
              </button>
            )}
            <button
              type="button"
              onClick={() => reset.mutate(u.id)}
              className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
              title="generate single-use reset URL"
            >
              <Cog size={10} /> reset pw
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

// ─── Audit ──────────────────────────────────────────────────────────────────

function AuditTab() {
  const q = useQuery({ queryKey: ["admin-audit"], queryFn: listAdminAudit });
  return (
    <ul className="space-y-1">
      {(q.data ?? []).map((r) => (
        <li key={r.id} className="mono flex items-center gap-2 rounded border border-white/10 bg-ink-subtle px-3 py-1.5 text-xs">
          <span className="text-chrome-dim">{r.occurred_at.slice(0, 19).replace("T", " ")}</span>
          <span className="text-chrome">@{r.actor_handle ?? "?"}</span>
          <span className="text-accent">{r.action}</span>
          {r.target_handle && <span className="text-chrome">→ @{r.target_handle}</span>}
          {r.ip && <span className="ml-auto text-chrome-dim">{r.ip}</span>}
        </li>
      ))}
      {q.data?.length === 0 && (
        <li className="mono text-xs text-chrome-dim">no admin events yet — clean conscience</li>
      )}
    </ul>
  );
}

// ─── Health ─────────────────────────────────────────────────────────────────

function HealthTab() {
  const q = useQuery({ queryKey: ["admin-health"], queryFn: getHealth, refetchInterval: 30_000 });
  if (!q.data) return <div className="mono text-xs text-chrome-dim">compiling vibes…</div>;
  const d = q.data;
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <HealthCard label="postgres" check={d.db} icon={Database} />
        <HealthCard label="redis" check={d.redis} icon={Server} />
        <HealthCard label="minio" check={d.minio} icon={Database} />
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-4">
        <StatTile label="version" value={<span className="mono">{d.version}</span>} />
        <StatTile label="jobs pending" value={d.jobs.pending} />
        <StatTile label="jobs running" value={d.jobs.running} />
        <StatTile
          label="jobs failed"
          value={d.jobs.failed}
          accent={d.jobs.failed > 0 ? "warn" : "good"}
        />
      </div>
    </div>
  );
}

function HealthCard({
  label, check, icon: Icon,
}: {
  label: string;
  check: { ok: boolean; latency_ms: number; detail: string | null };
  icon: React.ComponentType<{ size?: string | number }>;
}) {
  return (
    <div
      className={`rounded-lg border p-4 ${
        check.ok ? "border-emerald-500/30 bg-emerald-500/5" : "border-red-500/30 bg-red-500/5"
      }`}
    >
      <div className="mono mb-1 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
        <Icon size={11} /> {label}
      </div>
      <div className={`text-2xl ${check.ok ? "text-emerald-300" : "text-red-300"}`}>
        {check.ok ? "ok" : "down"}
      </div>
      <div className="mono mt-0.5 text-[10px] text-chrome-dim">
        {check.latency_ms}ms
      </div>
      {check.detail && (
        <div className="mono mt-1 text-[10px] text-red-200">{check.detail}</div>
      )}
    </div>
  );
}

// ─── Backups ────────────────────────────────────────────────────────────────

function BackupsTab() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["admin-backups"], queryFn: listBackups, refetchInterval: 10_000 });
  const start = useMutation({
    mutationFn: () => startBackup(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["admin-backups"] }),
  });

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => start.mutate()}
          disabled={start.isPending}
          className="mono inline-flex items-center gap-2 rounded bg-accent px-3 py-2 text-sm font-medium text-accent-fg hover:opacity-90 disabled:opacity-50"
        >
          <Download size={14} /> {start.isPending ? "spinning up…" : "$ pg_dump"}
        </button>
        <span className="mono text-[11px] text-chrome-dim">
          backups land in MinIO under <code>backups/YYYY-MM-DD/&lt;id&gt;.dump</code>
        </span>
      </div>
      <ul className="space-y-1">
        {(q.data ?? []).map((b) => (
          <li
            key={b.id}
            className="flex items-center gap-3 rounded border border-white/10 bg-ink-subtle px-3 py-2"
          >
            <span
              className={`mono inline-flex items-center rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-widest ${
                b.status === "done"
                  ? "border-emerald-500/30 text-emerald-300"
                  : b.status === "failed"
                    ? "border-red-500/30 text-red-300"
                    : b.status === "running"
                      ? "border-accent/40 text-accent"
                      : "border-white/10 text-chrome-dim"
              }`}
            >
              {b.status}
            </span>
            <span className="mono text-xs text-chrome-dim">
              {b.created_at.slice(0, 19).replace("T", " ")}
            </span>
            {b.storage_key && (
              <span className="mono ml-2 truncate text-xs text-chrome">{b.storage_key}</span>
            )}
            {b.size_bytes != null && (
              <span className="mono ml-auto text-xs text-chrome-dim">
                {(b.size_bytes / 1024 / 1024).toFixed(1)} MB
              </span>
            )}
            {b.error && (
              <span className="mono ml-auto truncate text-xs text-red-300">{b.error}</span>
            )}
          </li>
        ))}
        {q.data?.length === 0 && (
          <li className="mono rounded border border-dashed border-white/10 p-4 text-center text-xs text-chrome-dim">
            no backups yet — kick one off and the worker takes it from here
          </li>
        )}
      </ul>
      <p className="mono text-[10px] text-chrome-dim">
        restore is intentionally a manual operation. see <code>docs/SECURITY.md</code>.
      </p>
    </div>
  );
}

// ─── Webhooks (scaffolding) ─────────────────────────────────────────────────

function WebhooksTab() {
  return (
    <div className="rounded-lg border border-dashed border-white/10 bg-ink-subtle p-12 text-center">
      <Webhook size={28} className="mx-auto mb-3 text-chrome-dim" />
      <div className="mono mb-2 text-xs uppercase tracking-widest text-chrome-dim">
        webhooks · scaffolding only
      </div>
      <p className="text-sm text-chrome-dim">
        The schema and per-project CRUD exist (<code className="mono">/projects/:key/webhooks</code>),
        but outbound delivery isn&apos;t wired yet. Coming in the next pass.
      </p>
      <div className="mono mt-3 inline-flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-[10px] uppercase text-amber-300">
        <Check size={10} /> coming soon
      </div>
    </div>
  );
}
