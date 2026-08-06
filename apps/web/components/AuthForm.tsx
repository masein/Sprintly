"use client";

// Shared login/register form. Minimal, no shadcn yet — that lands when we
// have a real component library set up in M2. Voice per docs/PERSONALITY.md:
// monospace labels, no exclamation marks, error messages are honest.

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { useQueryClient } from "@tanstack/react-query";
import { enableRealtime } from "@/lib/ws";
import { markSignedIn } from "@/lib/session-signal";
import {
  login,
  register,
  twoFactorLogin,
  isTwoFactorChallenge,
  isMustChangePassword,
  changePasswordForced,
  requestPasswordReset,
  type ApiError,
} from "@/lib/auth-bundle";

type Mode = "login" | "register";

export function AuthForm({ mode }: { mode: Mode }) {
  const router = useRouter();
  const qc = useQueryClient();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [handle, setHandle] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [invite, setInvite] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Set once the password step succeeds but the account needs a second factor.
  const [challenge, setChallenge] = useState<string | null>(null);
  const [code, setCode] = useState("");
  // Set when a provisioned account must set a new password before getting in.
  const [resetChallenge, setResetChallenge] = useState<string | null>(null);
  const [newPassword, setNewPassword] = useState("");
  // "Forgot password" step: its own view, like the 2FA and forced-reset ones.
  const [forgot, setForgot] = useState(false);
  const [forgotSent, setForgotSent] = useState(false);

  // Minted invite links land here as /register?invite=<token> — prefill the
  // field so the invitee doesn't have to fish the token out of the URL.
  useEffect(() => {
    if (mode !== "register" || typeof window === "undefined") return;
    const t = new URLSearchParams(window.location.search).get("invite");
    if (t) setInvite(t);
  }, [mode]);

  function done() {
    // A session exists now — bring realtime up (and let the achievement
    // watcher start polling) without waiting for a reload; Providers only
    // does that for visitors who already had a session on load.
    markSignedIn();
    enableRealtime(qc);
    router.push("/");
    router.refresh();
  }

  async function onSubmitForgot(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await requestPasswordReset(email.trim());
      // The server answers the same way whether or not that address exists —
      // it won't confirm who has an account here — so the UI must not either.
      setForgotSent(true);
    } catch (err) {
      const e2 = err as ApiError;
      // A 429 is the only failure worth naming: it means "you've asked a lot".
      setError(
        e2?.status === 429
          ? "That's a lot of reset requests. Give it an hour."
          : e2?.message ?? "Couldn't send that. Try again in a moment.",
      );
    } finally {
      setSubmitting(false);
    }
  }

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      if (mode === "login") {
        const res = await login({ email, password });
        if (isTwoFactorChallenge(res)) {
          // Hold here — switch to the code step instead of navigating.
          setChallenge(res.challenge);
          return;
        }
        if (isMustChangePassword(res)) {
          // Provisioned account — force a new password before any session.
          setResetChallenge(res.challenge);
          return;
        }
      } else {
        await register({
          email,
          handle,
          display_name: displayName,
          password,
          invite_token: invite || undefined,
        });
      }
      done();
    } catch (err) {
      const apiErr = err as unknown as ApiError;
      setError(humanize(apiErr, mode));
    } finally {
      setSubmitting(false);
    }
  }

  async function onSubmit2fa(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await twoFactorLogin(challenge!, code.trim());
      done();
    } catch (err) {
      const apiErr = err as unknown as ApiError;
      setError(
        apiErr.code === "rate_limited"
          ? "Too many attempts. Wait a minute, then try again."
          : "That code didn't work. Try again, or use a recovery code.",
      );
    } finally {
      setSubmitting(false);
    }
  }

  async function onSubmitNewPassword(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await changePasswordForced(resetChallenge!, newPassword);
      done();
    } catch (err) {
      const apiErr = err as unknown as ApiError;
      setError(
        apiErr.code === "validation"
          ? "Pick a password of at least 10 characters."
          : "That didn't work — your reset link may have expired. Sign in again.",
      );
    } finally {
      setSubmitting(false);
    }
  }

  // Force-reset step: a provisioned account must set its own password first.
  if (resetChallenge) {
    return (
      <form onSubmit={onSubmitNewPassword} className="space-y-5">
        <p className="text-sm text-chrome-dim">
          This account was set up with a temporary password. Choose your own (at
          least 10 characters) to finish signing in.
        </p>
        <Field
          label="New password"
          type="password"
          value={newPassword}
          onChange={setNewPassword}
          autoComplete="new-password"
          minLength={10}
          autoFocus
          required
        />
        {error && (
          <div
            role="alert"
            className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
          >
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={submitting || newPassword.length < 10}
          className="mono w-full rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {submitting ? "setting…" : "$ set password"}
        </button>
      </form>
    );
  }

  // Second-factor step: shown after a correct password on a 2FA account.
  if (challenge) {
    return (
      <form onSubmit={onSubmit2fa} className="space-y-5">
        <p className="text-sm text-chrome-dim">
          Enter the 6-digit code from your authenticator app. Lost your phone?
          Type one of your recovery codes instead.
        </p>
        <Field
          label="Authentication code"
          value={code}
          onChange={setCode}
          placeholder="123456"
          autoComplete="one-time-code"
          mono
          autoFocus
          required
        />
        {error && (
          <div
            role="alert"
            className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
          >
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={submitting || !code.trim()}
          className="mono w-full rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {submitting ? "checking…" : "$ verify"}
        </button>
        <button
          type="button"
          onClick={() => {
            setChallenge(null);
            setCode("");
            setError(null);
          }}
          className="mono w-full text-xs text-chrome-dim hover:text-chrome"
        >
          ← back
        </button>
      </form>
    );
  }

  if (forgot) {
    return (
      <form onSubmit={onSubmitForgot} className="space-y-5" aria-label="reset password">
        <p className="text-sm text-chrome-dim">
          Give us the email on the account. If it exists, a reset link lands in
          the inbox — the link is good for 30 minutes, and once.
        </p>
        <Field
          label="Email"
          type="email"
          value={email}
          onChange={setEmail}
          autoComplete="email"
          required
        />
        {forgotSent && (
          <div
            role="status"
            data-testid="reset-sent"
            className="mono rounded border border-accent/30 bg-accent/10 p-3 text-sm text-chrome"
          >
            Sent, assuming that address has an account here. Check the inbox —
            and the spam folder, since this mail is new.
          </div>
        )}
        {error && (
          <div
            role="alert"
            className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
          >
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={submitting || !email.trim()}
          className="mono w-full rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {submitting ? "sending…" : "$ mail me a link"}
        </button>
        <button
          type="button"
          onClick={() => {
            setForgot(false);
            setForgotSent(false);
            setError(null);
          }}
          className="mono w-full text-xs text-chrome-dim hover:text-chrome"
        >
          ← back to sign in
        </button>
      </form>
    );
  }

  return (
    <form onSubmit={onSubmit} className="space-y-5">
      {mode === "register" && (
        <>
          <Field
            label="Display name"
            value={displayName}
            onChange={setDisplayName}
            placeholder="Your name"
            autoComplete="name"
            required
          />
          <Field
            label="Handle"
            value={handle}
            onChange={setHandle}
            placeholder="for @mentions, e.g. mohammad"
            autoComplete="username"
            mono
            required
          />
        </>
      )}

      <Field
        label="Email"
        type="email"
        value={email}
        onChange={setEmail}
        autoComplete={mode === "login" ? "username" : "email"}
        required
      />
      <Field
        label="Password"
        type="password"
        value={password}
        onChange={setPassword}
        autoComplete={mode === "login" ? "current-password" : "new-password"}
        minLength={mode === "register" ? 10 : 1}
        required
      />

      {mode === "register" && (
        <Field
          label="Invite token (optional)"
          value={invite}
          onChange={setInvite}
          placeholder="paste if your admin gave you one"
          mono
        />
      )}

      {error && (
        <div
          role="alert"
          className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
        >
          {error}
        </div>
      )}

      <button
        type="submit"
        disabled={submitting}
        className="mono w-full rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {submitting
          ? "nudging electrons…"
          : mode === "login"
            ? "$ ssh sprintly"
            : "$ git init account"}
      </button>

      {/* The reset endpoints shipped in M1; until now nothing in the UI
          reached them, so a forgotten password meant asking an admin. */}
      {mode === "login" && (
        <button
          type="button"
          onClick={() => {
            setForgot(true);
            setError(null);
          }}
          className="mono w-full text-xs text-chrome-dim hover:text-chrome"
        >
          forgot your password?
        </button>
      )}
    </form>
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
} & Omit<
  React.InputHTMLAttributes<HTMLInputElement>,
  "value" | "onChange"
>) {
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

function humanize(err: ApiError, mode: Mode): string {
  switch (err.code) {
    case "unauthorized":
      return "Email or password didn't match. Try again.";
    case "forbidden":
      return mode === "register"
        ? "Registration is closed. Ask an admin for an invite token."
        : "Your account isn't active. Contact an admin.";
    case "conflict":
      return "That email or handle is already taken.";
    case "validation":
      return "Some fields look off. Check the form.";
    default:
      return err.message;
  }
}
