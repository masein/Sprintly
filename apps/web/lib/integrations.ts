// Git integration: commits/PRs linked to a task.

import { api } from "./api";

export type GitLink = {
  id: string;
  kind: "commit" | "pull_request" | "branch";
  external_ref: string;
  url: string | null;
  title: string | null;
  state: string | null;
  created_at: string;
};

export const listGitLinks = (taskKey: string) =>
  api<GitLink[]>(`/tasks/${encodeURIComponent(taskKey)}/git-links`);
