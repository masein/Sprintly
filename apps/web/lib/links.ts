// Shareable URLs for things in the app. Built from whatever origin the app is
// open on, so a link copied over https://sprintly.example points there and one
// copied from a LAN address points at the LAN.

export function taskUrl(origin: string, key: string): string {
  return `${origin.replace(/\/+$/, "")}/tasks/${encodeURIComponent(key)}`;
}
