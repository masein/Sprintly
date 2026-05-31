"use client";

// Shared login/register form. Minimal, no shadcn yet — that lands when we
// have a real component library set up in M2. Voice per docs/PERSONALITY.md:
// monospace labels, no exclamation marks, error messages are honest.

import { useState } from "react";
import { useRouter } from "next/navigation";
import { login, register, type ApiError } from "@/lib/auth-bundle";

type Mode = "login" | "register";

export function AuthForm({ mode }: { mode: Mode }) {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [handle, setHandle] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [invite, setInvite] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      if (mode === "login") {
        await login({ email, password });
      } else {
        await register({
          email,
          handle,
          display_name: displayName,
          password,
          invite_token: invite || undefined,
        });
      }
      router.push("/");
      router.refresh();
    } catch (err) {
      const apiErr = err as unknown as ApiError;
      setError(humanize(apiErr, mode));
    } finally {
      setSubmitting(false);
    }
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
