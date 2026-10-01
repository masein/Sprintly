// Who may do what in the UI — the client mirror of the API's permission
// matrix (apps/api/src/domain/permissions.rs). Keep the two in step: a control
// the API refuses is a 403 waiting to happen.

type ProjectLike = {
  your_role?: "lead" | "contributor" | "watcher" | null;
  archived_at?: string | null;
};

/**
 * Task work — create, edit, move, comment, link, attach, plan. Leads and
 * contributors, plus global admins; never on an archived project, and never
 * for a global viewer (the API refuses them whatever their project role).
 */
export function canEditTasks(project: ProjectLike | null | undefined, globalRole?: string | null): boolean {
  if (!project || project.archived_at) return false;
  if (globalRole === "admin") return true;
  if (globalRole === "viewer") return false;
  return project.your_role === "lead" || project.your_role === "contributor";
}

/** Project configuration and deleting work: leads and global admins. */
export function canManageProject(project: ProjectLike | null | undefined, globalRole?: string | null): boolean {
  if (!project) return false;
  if (globalRole === "admin") return true;
  return project.your_role === "lead";
}
