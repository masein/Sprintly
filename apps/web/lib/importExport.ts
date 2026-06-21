// Import / export (F16).

import { api } from "./api";

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080/api/v1";

export type ImportFormat = "auto" | "trello" | "csv" | "jira";

export type ImportReport = {
  dry_run: boolean;
  source: string;
  columns_created: string[];
  columns_reused: string[];
  labels_created: string[];
  epics_created: string[];
  sprints_created: string[];
  fields_created: string[];
  tasks_created: number;
  tasks_updated: number;
  comments_created: number;
  warnings: string[];
};

export const importProject = (
  key: string,
  body: { format: ImportFormat; content: string; dry_run: boolean },
) =>
  api<ImportReport>(`/projects/${encodeURIComponent(key)}/import`, {
    method: "POST",
    body,
  });

export const exportUrl = (key: string, format: "json" | "csv") =>
  `${API_BASE}/projects/${encodeURIComponent(key)}/export?format=${format}`;
