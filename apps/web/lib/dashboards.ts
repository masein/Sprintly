// Dashboard API types + wrappers.

import { api } from "./api";

export type StatusCounts = {
  todo: number;
  in_progress: number;
  review: number;
  done: number;
};

export type CurrentSprintSummary = {
  id: string;
  name: string;
  starts_at: string;
  ends_at: string;
  total_points: number;
  done_points: number;
  task_count: number;
};

export type VelocityPoint = {
  sprint_id: string;
  name: string;
  completed_at: string | null;
  velocity_points: number;
};

export type ContributorRow = {
  user_id: string;
  handle: string;
  display_name: string;
  minutes: number;
};

export type ActivityRow = {
  id: string;
  task_key: string;
  kind: string;
  actor_handle: string | null;
  created_at: string;
};

export type BlockedSample = {
  task_key: string;
  title: string;
  blocked_by_count: number;
};

export type DueRow = {
  task_key: string;
  title: string;
  due_date: string;
  assignee_handle: string | null;
  days_until: number;
};

export type ProjectDashboard = {
  status_counts: StatusCounts;
  current_sprint: CurrentSprintSummary | null;
  velocity_history: VelocityPoint[];
  top_contributors: ContributorRow[];
  recent_activity: ActivityRow[];
  blocked: { count: number; samples: BlockedSample[] };
  upcoming_due: DueRow[];
  time_this_week_minutes: number;
};

export const getProjectDashboard = (projectKey: string) =>
  api<ProjectDashboard>(
    `/projects/${encodeURIComponent(projectKey)}/dashboard`,
  );

export type MyTaskSample = {
  key: string;
  project_key: string;
  title: string;
  status: string;
  priority: string;
};

export type WatchedRow = {
  task_key: string;
  title: string;
  last_activity_at: string;
  last_kind: string;
};

export type RunningTimerRef = {
  task_key: string;
  started_at: string;
};

export type MyDashboard = {
  my_status_counts: StatusCounts;
  overdue: DueRow[];
  my_tasks_sample: MyTaskSample[];
  watched_changed_recently: WatchedRow[];
  time_this_week_minutes: number;
  running_timer: RunningTimerRef | null;
};

export const getMyDashboard = () => api<MyDashboard>("/me/dashboard");
