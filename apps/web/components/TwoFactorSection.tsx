"use client";

// Settings section: two-factor auth (F11). Enrol shows a QR + secret, verifies
// one code to switch it on, then reveals single-use recovery codes exactly
// once. Disabling needs a current code (or a recovery code).

import { useEffect, useState } from "react";
import { QRCodeSVG } from "qrcode.react";
import { Check, Copy, ShieldCheck, ShieldOff } from "lucide-react";
import {
  activateTwoFactor,
  disableTwoFactor,
  enrollTwoFactor,
  getTwoFactorStatus,
  type EnrollResponse,
  type TwoFactorStatus,
} from "@/lib/twoFactor";
import type { ApiError } from "@/lib/api";

export function TwoFactorSection() {
  const [status, setStatus] = useState<TwoFactorStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Enrolment state.
  const [enroll, setEnroll] = useState<EnrollResponse | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [recovery, setRecovery] = useState<string[] | null>(null);
  const [copied, setCopied] = useState(false);

  // Disable state.
  const [disableCode, setDisableCode] = useState("");

  async function reload() {
    try {
      setStatus(await getTwoFactorStatus());
    } catch (e) {
      setError((e as unknown as ApiError).message);
    }
  }
  useEffect(() => {
    void reload();
  }, []);

  async function startEnroll() {
    setError(null);
    setBusy(true);
    try {
      setEnroll(await enrollTwoFactor());
    } catch (e) {
      setError((e as unknown as ApiError).message);
    } finally {
      setBusy(false);
    }
  }

  async function confirmEnroll(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const res = await activateTwoFactor(code.trim());
      setRecovery(res.recovery_codes);
      setEnroll(null);
      setCode("");
      await reload();
    } catch (err) {
      const apiErr = err as unknown as ApiError;
      setError(
        apiErr.code === "rate_limited"
          ? "Too many attempts. Wait a minute and try again."
          : "That code didn't match. Check your app's clock and try the current code.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function turnOff(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await disableTwoFactor(disableCode.trim());
      setDisableCode("");
      await reload();
    } catch (err) {
      const apiErr = err as unknown as ApiError;
      setError(
        apiErr.code === "unauthorized"
          ? "That code didn't work. Use a current code or a recovery code."
          : (apiErr.message ?? "failed"),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="space-y-3 border-t border-white/10 pt-6">
      <div>
        <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
          {status?.enabled ? <ShieldCheck size={12} /> : <ShieldOff size={12} />}
          two-factor auth
        </h2>
        <p className="mt-1 text-xs text-chrome-dim">
          A time-based code from an authenticator app, required at sign-in on top
          of your password.
          {status?.required && !status.enabled && (
            <span className="text-amber-300"> Your admin asks everyone to turn this on.</span>
          )}
        </p>
      </div>

      {/* Recovery codes — shown exactly once, right after enabling. */}
      {recovery && (
        <div className="space-y-2 rounded border border-accent/40 bg-accent/10 p-3">
          <div className="mono text-[11px] uppercase tracking-widest text-chrome-dim">
            save these recovery codes — each works once, and we won&apos;t show them again
          </div>
          <div className="grid grid-cols-2 gap-1">
            {recovery.map((c) => (
              <code key={c} className="mono text-xs text-chrome">{c}</code>
            ))}
          </div>
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={async () => {
                await navigator.clipboard.writeText(recovery.join("\n"));
                setCopied(true);
              }}
              className="mono inline-flex items-center gap-1 text-[11px] text-accent hover:underline"
            >
              {copied ? <Check size={12} /> : <Copy size={12} />}
              {copied ? "copied" : "copy all"}
            </button>
            <button
              type="button"
              onClick={() => setRecovery(null)}
              className="mono text-[11px] text-chrome-dim hover:text-chrome"
            >
              I&apos;ve saved them — done
            </button>
          </div>
        </div>
      )}

      {/* State: ON */}
      {status?.enabled && !recovery && (
        <form onSubmit={turnOff} className="flex flex-wrap items-center gap-2">
          <span className="mono inline-flex items-center gap-1 rounded border border-accent/40 px-2 py-0.5 text-[11px] text-accent">
            <ShieldCheck size={11} /> on
          </span>
          <input
            value={disableCode}
            onChange={(e) => setDisableCode(e.target.value)}
            placeholder="code to turn off"
            aria-label="code to disable two-factor"
            className="mono w-40 rounded border border-white/10 bg-ink-subtle px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
          />
          <button
            type="submit"
            disabled={busy || !disableCode.trim()}
            className="mono rounded border border-white/10 px-3 py-1 text-xs text-chrome-dim hover:border-red-400/40 hover:text-red-300 disabled:opacity-50"
          >
            disable
          </button>
        </form>
      )}

      {/* State: OFF, not yet enrolling */}
      {status && !status.enabled && !enroll && !recovery && (
        <button
          type="button"
          onClick={startEnroll}
          disabled={busy}
          className="mono rounded bg-accent px-3 py-1.5 text-xs text-accent-fg disabled:opacity-50"
        >
          {busy ? "preparing…" : "set up two-factor"}
        </button>
      )}

      {/* State: OFF, enrolling — show QR + secret + confirm */}
      {enroll && (
        <div className="space-y-3 rounded border border-white/10 bg-ink-subtle p-3">
          <p className="text-xs text-chrome-dim">
            Scan this with your authenticator app (or enter the key by hand), then
            type the 6-digit code it shows to confirm.
          </p>
          <div className="flex flex-wrap items-center gap-4">
            <div className="rounded bg-white p-2">
              <QRCodeSVG value={enroll.otpauth_uri} size={140} />
            </div>
            <div className="space-y-1">
              <div className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
                setup key
              </div>
              <code
                data-testid="totp-secret"
                className="mono block max-w-[16rem] break-all text-xs text-chrome"
              >
                {enroll.secret}
              </code>
            </div>
          </div>
          <form onSubmit={confirmEnroll} className="flex items-center gap-2">
            <input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="123456"
              aria-label="confirmation code"
              autoComplete="one-time-code"
              className="mono w-32 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
            />
            <button
              type="submit"
              disabled={busy || !code.trim()}
              className="mono rounded bg-accent px-3 py-1 text-xs text-accent-fg disabled:opacity-50"
            >
              verify & turn on
            </button>
            <button
              type="button"
              onClick={() => {
                setEnroll(null);
                setCode("");
                setError(null);
              }}
              className="mono text-[11px] text-chrome-dim hover:text-chrome"
            >
              cancel
            </button>
          </form>
        </div>
      )}

      {error && <div className="mono text-[11px] text-red-300">{error}</div>}
    </section>
  );
}
