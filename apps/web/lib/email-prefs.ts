// Per-user email delivery preferences. The unsubscribe token lives server-side
// only — it's never part of this payload.

import { api } from "./api";

export type EmailMode = "off" | "immediate" | "digest";

export type EmailPrefs = {
  mode: EmailMode;
  kinds: Record<string, boolean>;
  digest_hour: number;
  available_kinds: string[];
  /** False when the operator hasn't configured SMTP — mail is only logged. */
  delivery_configured: boolean;
};

export const getEmailPrefs = () => api<EmailPrefs>("/users/me/email-prefs");

export const patchEmailPrefs = (body: {
  mode?: EmailMode;
  /** Partial — only the kinds named are changed. */
  kinds?: Record<string, boolean>;
  digest_hour?: number;
}) => api<EmailPrefs>("/users/me/email-prefs", { method: "PATCH", body });

/** What each kind means, in the user's terms rather than the schema's. */
export const KIND_LABELS: Record<string, string> = {
  mention: "someone @mentions me",
  assigned: "a task is assigned to me",
  comment: "someone comments on a task I watch",
};
