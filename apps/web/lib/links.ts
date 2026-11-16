// Shareable URLs for things in the app. Built from whatever origin the app is
// open on, so a link copied over https://sprintly.example points there and one
// copied from a LAN address points at the LAN.

export function taskUrl(origin: string, key: string): string {
  return `${origin.replace(/\/+$/, "")}/tasks/${encodeURIComponent(key)}`;
}

/**
 * `raw` if it's an absolute http(s) URL, else `null` — for rendering links
 * whose target came from outside (git provider payloads, user input). React
 * doesn't block `javascript:` hrefs, so an unchecked `<a href={url}>` runs
 * script in whoever clicks it, on the app's origin.
 */
export function safeExternalUrl(raw: string | null | undefined): string | null {
  if (!raw) return null;
  try {
    const u = new URL(raw.trim());
    return u.protocol === "https:" || u.protocol === "http:" ? u.href : null;
  } catch {
    return null; // relative or unparseable — not an external link
  }
}
