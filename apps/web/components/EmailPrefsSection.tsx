"use client";

// Settings → email. Three questions: whether we mail you, what we mail about,
// and (for a digest) when. Voice per docs/PERSONALITY.md — and honest when the
// operator hasn't configured SMTP, because otherwise these switches look
// broken rather than unplugged.

import { useEffect, useState } from "react";
import { Mail } from "lucide-react";
import {
  getEmailPrefs,
  patchEmailPrefs,
  KIND_LABELS,
  type EmailMode,
  type EmailPrefs,
} from "@/lib/email-prefs";
import type { ApiError } from "@/lib/api";

const MODES: { value: EmailMode; label: string; hint: string }[] = [
  { value: "immediate", label: "as it happens", hint: "one email per notification" },
  { value: "digest", label: "once a day", hint: "everything in one email" },
  { value: "off", label: "never", hint: "the in-app bell still works" },
];

export function EmailPrefsSection() {
  const [prefs, setPrefs] = useState<EmailPrefs | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<Date | null>(null);

  useEffect(() => {
    let alive = true;
    getEmailPrefs()
      .then((p) => alive && setPrefs(p))
      .catch((e: ApiError) => alive && setError(e?.message ?? "Couldn't load your email settings."))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, []);

  async function patch(body: Parameters<typeof patchEmailPrefs>[0]) {
    setError(null);
    // Optimistic: these are toggles, and waiting on a round-trip to move a
    // checkbox feels broken.
    const previous = prefs;
    setPrefs((p) => (p ? { ...p, ...body, kinds: { ...p.kinds, ...(body.kinds ?? {}) } } : p));
    try {
      setPrefs(await patchEmailPrefs(body));
      setSavedAt(new Date());
    } catch (e) {
      setPrefs(previous);
      setError((e as ApiError)?.message ?? "That didn't save.");
    }
  }

  if (loading) {
    return (
      <section className="space-y-3">
        <SectionTitle />
        <p className="mono text-xs text-chrome-dim">loading…</p>
      </section>
    );
  }
  if (!prefs) {
    return (
      <section className="space-y-3">
        <SectionTitle />
        <p role="alert" className="mono text-xs text-red-300">
          {error ?? "Couldn't load your email settings."}
        </p>
      </section>
    );
  }

  return (
    <section className="space-y-4" aria-label="email notifications">
      <SectionTitle />

      {!prefs.delivery_configured && (
        <p className="mono rounded border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-200">
          No mail server is configured on this deployment yet, so these emails
          are written to the server log instead of sent. Your choices are kept
          either way.
        </p>
      )}

      <fieldset className="space-y-2">
        <legend className="mono text-xs uppercase tracking-widest text-chrome-dim">
          Email me
        </legend>
        <div className="flex flex-wrap gap-2">
          {MODES.map((m) => (
            <button
              key={m.value}
              type="button"
              aria-pressed={prefs.mode === m.value}
              onClick={() => void patch({ mode: m.value })}
              title={m.hint}
              className={`mono rounded border px-3 py-1.5 text-xs transition ${
                prefs.mode === m.value
                  ? "border-accent bg-accent/10 text-chrome"
                  : "border-white/10 text-chrome-dim hover:border-white/20"
              }`}
            >
              {m.label}
            </button>
          ))}
        </div>
      </fieldset>

      {prefs.mode !== "off" && (
        <>
          <fieldset className="space-y-2">
            <legend className="mono text-xs uppercase tracking-widest text-chrome-dim">
              About
            </legend>
            <div className="space-y-1.5">
              {prefs.available_kinds.map((k) => (
                <label key={k} className="mono flex items-center gap-2 text-sm text-chrome">
                  <input
                    type="checkbox"
                    checked={!!prefs.kinds[k]}
                    onChange={(e) => void patch({ kinds: { [k]: e.target.checked } })}
                    className="accent-accent"
                  />
                  {KIND_LABELS[k] ?? k}
                </label>
              ))}
            </div>
          </fieldset>

          {prefs.mode === "digest" && (
            <label className="mono block space-y-1.5 text-xs text-chrome-dim">
              <span className="uppercase tracking-widest">Send it around</span>
              <select
                aria-label="digest hour"
                value={prefs.digest_hour}
                onChange={(e) => void patch({ digest_hour: Number(e.target.value) })}
                className="block rounded border border-white/10 bg-ink px-2 py-1.5 text-sm text-chrome outline-none focus:border-accent"
              >
                {Array.from({ length: 24 }, (_, h) => (
                  <option key={h} value={h}>
                    {String(h).padStart(2, "0")}:00
                  </option>
                ))}
              </select>
              <span className="block text-[11px]">
                Your local time — set your timezone above and this follows it.
              </span>
            </label>
          )}
        </>
      )}

      {error && (
        <p role="alert" className="mono text-xs text-red-300">
          {error}
        </p>
      )}
      {savedAt && !error && (
        <p className="mono text-xs text-chrome-dim">email settings saved {savedAt.toLocaleTimeString()}</p>
      )}
    </section>
  );
}

function SectionTitle() {
  return (
    <h2 className="mono flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim">
      <Mail size={11} /> Email
    </h2>
  );
}
