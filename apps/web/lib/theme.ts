// Theme management. Five named themes; choice persists in localStorage and
// applies as `<html data-theme="…">`. Defaults to midnight to honor the spec.

export const THEMES = [
  "midnight",
  "daylight",
  "solarized_dusk",
  "terminal_green",
  "hot_pink",
] as const;

export type Theme = (typeof THEMES)[number];

const STORAGE_KEY = "sprintly:theme";

export function getTheme(): Theme {
  if (typeof document === "undefined") return "midnight";
  const stored = (localStorage.getItem(STORAGE_KEY) ?? "") as Theme;
  return (THEMES as readonly string[]).includes(stored) ? stored : "midnight";
}

export function setTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", theme);
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    /* private mode etc. — never throw */
  }
}

/** Call once on app boot to hydrate the saved theme. */
export function applyStoredTheme(): void {
  setTheme(getTheme());
}
