// Clipboard that works on plain-HTTP deployments too.
//
// `navigator.clipboard` only exists in secure contexts (https or localhost).
// Self-hosted Sprintly often runs on a bare IP over http — where every copy
// button in the app silently died (or crashed its handler outright). This
// helper tries the real API first and falls back to the legacy
// textarea + execCommand trick. Returns whether the copy landed, so callers
// can be honest in their UI instead of assuming success.

export async function copyText(text: string): Promise<boolean> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Secure-context API refused (permissions, focus) — try the fallback.
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

// ── Auto-wipe for copied secrets ─────────────────────────────────────────────
//
// Module-scoped on purpose. The vault used to keep this timer in the row
// component and cancel it on unmount — so copying a secret and then leaving
// the vault page meant the clipboard was never cleared. A module-level timer
// outlives the component and the route.

let wipeTimer: ReturnType<typeof setTimeout> | null = null;
let focusRetry: (() => void) | null = null;

/**
 * Overwrite the clipboard `ms` from now. If the browser refuses at that
 * moment — writes need a focused page, and you've usually switched apps to
 * paste — try again as soon as this page regains focus. (That retry can, in
 * principle, clobber something you copied elsewhere in the meantime; for a
 * secret, failing towards "cleared" is the right side to err on.)
 */
export function scheduleClipboardWipe(ms: number): void {
  cancelClipboardWipe();
  wipeTimer = setTimeout(() => {
    wipeTimer = null;
    void wipeNowOrOnFocus();
  }, ms);
}

/** Drop any pending wipe (e.g. a newer copy rescheduled it). */
export function cancelClipboardWipe(): void {
  if (wipeTimer) clearTimeout(wipeTimer);
  wipeTimer = null;
  if (focusRetry && typeof window !== "undefined") {
    window.removeEventListener("focus", focusRetry);
  }
  focusRetry = null;
}

async function wipeNowOrOnFocus(): Promise<void> {
  if (await copyText("")) return;
  if (typeof window === "undefined") return;
  const retry = () => {
    window.removeEventListener("focus", retry);
    if (focusRetry === retry) focusRetry = null;
    void copyText("");
  };
  focusRetry = retry;
  window.addEventListener("focus", retry);
}
