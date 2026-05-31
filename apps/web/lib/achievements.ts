// Achievements API surface + the local-state toast queue.

import { api } from "./api";

export type CatalogRow = {
  code: string;
  title: string;
  description: string;
  icon: string;
};

export type AwardedRow = CatalogRow & {
  awarded_at: string;
  context: Record<string, unknown>;
};

export const listCatalog = () =>
  api<{ items: CatalogRow[] }>("/achievements").then((r) => r.items);

export const listMyAchievements = () =>
  api<{ items: AwardedRow[] }>("/me/achievements").then((r) => r.items);

export const triggerRtfm = () =>
  api<{ awarded: boolean }>("/me/achievements/rtfm", { method: "POST" });
