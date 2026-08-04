// Local-time helpers for the time-tracking UI.
//
// Time logs are stored as instants (timestamptz) — correct. The UI then
// rendered them with `toISOString()`, i.e. in UTC, which is why a 09:00 entry
// made in Tehran displayed as "05:30" and looked like a meaningless
// placeholder. Everything the user sees or types is in *their* zone; only the
// wire format is UTC.

/** Today, as YYYY-MM-DD in the viewer's timezone (not UTC). */
export function localDateISO(d: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Now, as HH:MM in the viewer's timezone. */
export function localTimeHHMM(d: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}`;
}

/**
 * A local date + HH:MM (as typed into date/time inputs) → the UTC instant to
 * send. `new Date("YYYY-MM-DDTHH:MM")` is parsed as *local* time by spec,
 * which is exactly what we want here.
 */
export function localToUtcISO(dateISO: string, timeHHMM: string): string {
  const hhmm = /^\d{2}:\d{2}$/.test(timeHHMM) ? timeHHMM : "09:00";
  return new Date(`${dateISO}T${hhmm}:00`).toISOString();
}

/** An instant → "YYYY-MM-DD HH:MM" in the viewer's timezone. */
export function fmtLocal(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${localDateISO(d)} ${localTimeHHMM(d)}`;
}
