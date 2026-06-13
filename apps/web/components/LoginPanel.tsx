"use client";

// The interactive half of the login page (F10): password form, SSO button, and
// any SSO error bounced back from the callback. We fetch the OIDC status so the
// button only shows when configured and the password form hides when the org
// has gone SSO-only.

import { useEffect, useState } from "react";
import { AuthForm } from "@/components/AuthForm";
import { getOidcStatus, oidcStartUrl, type OidcStatus } from "@/lib/oidc";

const SSO_ERRORS: Record<string, string> = {
  denied: "Your identity provider declined, or your email isn't allowed here.",
  expired: "That sign-in took too long. Start again.",
  failed: "Single sign-on didn't complete. Try again.",
};

export function LoginPanel({ ssoError }: { ssoError: string | null }) {
  const [status, setStatus] = useState<OidcStatus | null>(null);

  useEffect(() => {
    void getOidcStatus()
      .then(setStatus)
      .catch(() => setStatus({ enabled: false, local_login_disabled: false }));
  }, []);

  // Until we know, show the password form (the common case) — but don't flash
  // an SSO button we might hide.
  const showPassword = !status?.local_login_disabled;
  const showSso = status?.enabled ?? false;

  return (
    <div className="space-y-6">
      {ssoError && (
        <div
          role="alert"
          className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
        >
          {SSO_ERRORS[ssoError] ?? SSO_ERRORS.failed}
        </div>
      )}

      {showPassword && <AuthForm mode="login" />}

      {showSso && (
        <div className="space-y-3">
          {showPassword && (
            <div className="flex items-center gap-3">
              <div className="h-px flex-1 bg-white/10" />
              <span className="mono text-[10px] uppercase tracking-widest text-chrome-dim">
                or
              </span>
              <div className="h-px flex-1 bg-white/10" />
            </div>
          )}
          <a
            href={oidcStartUrl()}
            className="mono block w-full rounded border border-white/10 px-4 py-2 text-center text-sm text-chrome transition hover:border-white/20 hover:bg-white/5"
          >
            $ sso --login
          </a>
        </div>
      )}
    </div>
  );
}
