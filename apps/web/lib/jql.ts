// Query-language search over tasks, plus saved queries ("templates").
// The language itself is parsed on the server (apps/api/src/domain/jql.rs) —
// this is only the transport.

import { api } from "./api";

export type JqlHit = {
  key: string;
  project_key: string;
  title: string;
  status: string;
  priority: string;
  type: string;
  assignee_handle: string | null;
  sprint_name: string | null;
  labels: string[];
  story_points: number | null;
  due_date: string | null;
  updated_at: string;
};

export type JqlResult = { items: JqlHit[]; total: number };

export type SavedQuery = {
  id: string;
  name: string;
  jql: string;
  is_shared: boolean;
  /** False for a teammate's shared query — you can run it, not rewrite it. */
  is_mine: boolean;
  owner_handle: string;
  created_at: string;
  updated_at: string;
};

export const runJql = (jql: string, limit = 50, offset = 0) =>
  api<JqlResult>(
    `/search/jql?jql=${encodeURIComponent(jql)}&limit=${limit}&offset=${offset}`,
  );

export const listSavedQueries = () =>
  api<{ items: SavedQuery[] }>("/search/queries").then((r) => r.items);

export const createSavedQuery = (body: {
  name: string;
  jql: string;
  is_shared?: boolean;
}) => api<SavedQuery>("/search/queries", { method: "POST", body });

export const updateSavedQuery = (
  id: string,
  body: { name?: string; jql?: string; is_shared?: boolean },
) => api<SavedQuery>(`/search/queries/${id}`, { method: "PATCH", body });

export const deleteSavedQuery = (id: string) =>
  api<void>(`/search/queries/${id}`, { method: "DELETE" });

/** Every field the language knows, for the cheatsheet. */
export const JQL_FIELDS = [
  "key",
  "project",
  "title",
  "description",
  "text",
  "status",
  "priority",
  "type",
  "assignee",
  "reporter",
  "label",
  "sprint",
  "epic",
  "parent",
  "points",
  "estimate",
  "due",
  "created",
  "updated",
  "completed",
] as const;

/** Worked examples — the fastest way to learn a query language is to edit one. */
export const JQL_EXAMPLES: { label: string; jql: string }[] = [
  {
    label: "my open work",
    jql: "assignee = currentUser() AND status != done ORDER BY priority ASC",
  },
  { label: "unassigned in this sprint", jql: "sprint is not empty AND assignee is empty" },
  { label: "overdue", jql: "due < today AND status != done ORDER BY due ASC" },
  { label: "p0/p1 bugs", jql: "type = bug AND priority in (p0, p1)" },
  { label: "touched this week", jql: "updated >= -7d ORDER BY updated DESC" },
  { label: "big and unestimated", jql: "points >= 5 AND estimate is empty" },
  { label: "mentions login", jql: 'text ~ "login" ORDER BY updated DESC' },
];
