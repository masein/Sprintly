// Small display-formatting helpers shared across the UI.

/**
 * Count with a correctly-pluralized noun: `pluralize(1, "task")` → "1 task",
 * `pluralize(2, "task")` → "2 tasks". Pass an explicit `plural` for irregular
 * nouns (e.g. `pluralize(n, "entry", "entries")`).
 */
export function pluralize(n: number, singular: string, plural = `${singular}s`): string {
  return `${n} ${Math.abs(n) === 1 ? singular : plural}`;
}
