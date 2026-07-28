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
