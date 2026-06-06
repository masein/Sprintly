"use client";

// Password-reset landing page. The reset email links here with ?token=…; the
// user sets a new password, which we POST to /auth/password/reset/confirm.

import { Suspense, useState } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { confirmPasswordReset } from "@/lib/auth";
import type { ApiError } from "@/lib/api";

export default function ResetPage() {
  return (
    <Suspense fallback={null}>
      <ResetInner />
    </Suspense>
  );
}

function ResetInner() {
  const router = useRouter();
  const token = useSearchParams()?.get("token") ?? "";

  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [done, setDone] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (password !== confirm) {
      setError("Passwords don't match.");
      return;
    }
    setSubmitting(true);
    try {
      await confirmPasswordReset(token, password);
      setDone(true);
      setTimeout(() => router.push("/login"), 1500);
    } catch (e) {
      setError((e as unknown as ApiError).message ?? "That link is invalid or expired.");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-8 px-6 py-20">
      <header className="space-y-2">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · auth
        </div>
        <h1 className="text-3xl font-semibold">Set a new password.</h1>
        <p className="text-sm text-chrome-dim">
          Pick something your password manager will remember for you.
        </p>
      </header>

      {!token ? (
        <div className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200">
          This reset link is missing its token. Request a new one from the sign-in page.
        </div>
      ) : done ? (
        <div className="mono rounded border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm text-emerald-200">
          Password updated. Redirecting to sign in…
        </div>
      ) : (
        <form onSubmit={submit} className="space-y-4">
          <label className="block space-y-1.5">
            <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
              New password
            </span>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={10}
              maxLength={200}
              autoComplete="new-password"
              className="block w-full rounded border border-white/10 bg-ink px-3 py-2 text-sm text-chrome outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            />
          </label>
          <label className="block space-y-1.5">
            <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
              Confirm password
            </span>
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              required
              autoComplete="new-password"
              className="block w-full rounded border border-white/10 bg-ink px-3 py-2 text-sm text-chrome outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            />
          </label>

          {error && (
            <div className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200">
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={submitting || !password || !confirm}
            className="mono w-full rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {submitting ? "updating…" : "$ reset password"}
          </button>
        </form>
      )}

      <footer className="mono text-xs text-chrome-dim">
        remembered it?{" "}
        <Link href="/login" className="text-accent hover:underline">
          sign in
        </Link>
      </footer>
    </main>
  );
}
