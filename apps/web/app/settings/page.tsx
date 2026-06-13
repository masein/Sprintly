"use client";

// /settings — self-service profile editing. Bounces to /login on 401.
//
// This is the second authed page (after the landing badge). M9 fleshes out
// the theme/sound prefs; for now we expose display_name, timezone, and a
// theme picker stub that writes to settings.theme.

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { me, type Me, type ApiError } from "@/lib/auth-bundle";
import { api } from "@/lib/api";
import { ApiTokensSection } from "@/components/ApiTokensSection";
import { TwoFactorSection } from "@/components/TwoFactorSection";
import { setTheme as applyTheme, type Theme } from "@/lib/theme";

const THEMES = ["midnight", "daylight", "solarized_dusk", "terminal_green", "hot_pink"] as const;
type ThemeId = (typeof THEMES)[number];

export default function SettingsPage() {
  const router = useRouter();
  const [user, setUser] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<Date | null>(null);

  const [displayName, setDisplayName] = useState("");
  const [timezone, setTimezone] = useState("UTC");
  const [theme, setTheme] = useState<ThemeId>("midnight");

  useEffect(() => {
    let alive = true;
    me()
      .then((u) => {
        if (!alive) return;
        setUser(u);
        setDisplayName(u.display_name);
        setTimezone(u.timezone);
        const t = (u.settings?.theme as ThemeId | undefined) ?? "midnight";
        setTheme(THEMES.includes(t) ? t : "midnight");
      })
      .catch((e: ApiError) => {
        if (!alive) return;
        if (e.status === 401) {
          router.push("/login");
        } else {
          setError(e.message);
        }
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [router]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!user) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await api<Me>("/users/me", {
        method: "PATCH",
        body: {
          display_name: displayName,
          timezone,
          settings: { ...(user.settings ?? {}), theme },
        },
      });
      setUser(updated);
      setSavedAt(new Date());
    } catch (e) {
      setError((e as unknown as ApiError).message);
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return (
      <Shell>
        <div className="mono text-xs text-chrome-dim">compiling vibes…</div>
      </Shell>
    );
  }

  if (!user) {
    return (
      <Shell>
        <div className="mono text-sm text-chrome-dim">
          You&apos;re not signed in.{" "}
          <Link href="/login" className="text-accent hover:underline">
            sign in
          </Link>
          .
        </div>
      </Shell>
    );
  }

  return (
    <Shell>
      <header className="space-y-2">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · settings
        </div>
        <h1 className="text-3xl font-semibold">Your profile.</h1>
        <p className="mono text-xs text-chrome-dim">
          @{user.handle} · {user.role} · joined{" "}
          {new Date(user.created_at).toISOString().slice(0, 10)}
        </p>
      </header>

      <form onSubmit={save} className="space-y-5">
        <Field
          label="Display name"
          value={displayName}
          onChange={setDisplayName}
          required
        />
        <Field
          label="Timezone"
          value={timezone}
          onChange={setTimezone}
          placeholder="e.g. America/New_York"
          mono
        />

        <div className="space-y-1.5">
          <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
            Theme (preview — full set lands in M9)
          </span>
          <div className="flex flex-wrap gap-2">
            {THEMES.map((t) => (
              <button
                type="button"
                key={t}
                onClick={() => {
                  setTheme(t);
                  applyTheme(t as Theme);
                }}
                className={`mono rounded border px-3 py-1.5 text-xs transition ${
                  theme === t
                    ? "border-accent bg-accent/10 text-chrome"
                    : "border-white/10 text-chrome-dim hover:border-white/20"
                }`}
              >
                {t.replace("_", " ")}
              </button>
            ))}
          </div>
        </div>

        {error && (
          <div
            role="alert"
            className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
          >
            {error}
          </div>
        )}

        <div className="flex items-center gap-4">
          <button
            type="submit"
            disabled={saving}
            className="mono rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {saving ? "git push origin main…" : "$ save"}
          </button>
          {savedAt && (
            <span className="mono text-xs text-chrome-dim">
              saved {savedAt.toLocaleTimeString()}
            </span>
          )}
          <Link
            href="/"
            className="mono ml-auto text-xs text-chrome-dim hover:text-chrome"
          >
            ← back
          </Link>
        </div>
      </form>

      <TwoFactorSection />

      <ApiTokensSection />
    </Shell>
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main className="mx-auto flex min-h-screen max-w-2xl flex-col gap-8 px-6 py-16">
      {children}
    </main>
  );
}

function Field({
  label,
  value,
  onChange,
  mono = false,
  ...props
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  mono?: boolean;
} & Omit<React.InputHTMLAttributes<HTMLInputElement>, "value" | "onChange">) {
  return (
    <label className="block space-y-1.5">
      <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
        {label}
      </span>
      <input
        {...props}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={`block w-full rounded border border-white/10 bg-ink-subtle px-3 py-2 text-sm text-chrome outline-none transition focus:border-accent focus:ring-1 focus:ring-accent ${
          mono ? "mono" : ""
        }`}
      />
    </label>
  );
}
