//! RBAC chokepoint.
//!
//! Two axes:
//!   • Global role: admin / member / viewer  — from `users.role`.
//!   • Project role (per project membership): lead / contributor / watcher.
//!
//! A global admin bypasses project-role checks. Everyone else needs a
//! project role to do anything inside that project.
//!
//! Adding an `Action` variant without matching it below is a compile error.
//! That's deliberate.

use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    // user / admin surface
    InviteUser,
    SuspendUser,
    DeleteUser,
    ResetAnyPassword,
    ViewUser,
    EditOwnProfile,
    ViewAdminPanel,
    ViewAuditLog,
    TriggerBackup,

    // projects
    CreateProject,
    ViewProject,
    EditProject,
    ArchiveProject,
    AddProjectMember,
    RemoveProjectMember,
    ChangeProjectMemberRole,

    // boards & columns
    ViewBoard,
    ManageBoards,   // create/edit/delete boards
    ManageColumns,  // create/edit/delete/reorder columns
}

/// What's being acted upon.
#[derive(Debug, Clone, Copy)]
pub enum Resource {
    SelfRef,
    User(Uuid),
    Admin,
    /// A specific project. The caller passes the actor's role in that project
    /// (None = not a member). Global admins skip the lookup; routes don't
    /// even need to fetch the row.
    Project {
        id: Uuid,
        actor_role: Option<ProjectRole>,
        archived: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Actor {
    pub id: Uuid,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Member,
    Viewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRole {
    Lead,
    Contributor,
    Watcher,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }
}

impl ProjectRole {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lead" => Some(Self::Lead),
            "contributor" => Some(Self::Contributor),
            "watcher" => Some(Self::Watcher),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Contributor => "contributor",
            Self::Watcher => "watcher",
        }
    }
}

/// The single permission check.
pub fn can(actor: &Actor, action: Action, resource: Resource) -> bool {
    use Action as A;
    use ProjectRole as PR;
    use Resource as R;
    use Role::*;

    // Global admin: anything, anywhere.
    if actor.role == Admin {
        return true;
    }

    match (actor.role, action, resource) {
        // ── self & user surface ─────────────────────────────────────────
        (Member | Viewer, A::ViewUser, R::SelfRef) => true,
        (Member | Viewer, A::ViewUser, R::User(id)) => id == actor.id,
        (Member, A::EditOwnProfile, R::SelfRef) => true,
        (Viewer, A::EditOwnProfile, _) => false,

        // ── projects ────────────────────────────────────────────────────
        // Any active global member can create a new project.
        (Member, A::CreateProject, _) => true,

        // View: any project role can view; archived doesn't block reads.
        (Member | Viewer, A::ViewProject, R::Project { actor_role: Some(_), .. }) => true,

        (Member | Viewer, A::ViewBoard, R::Project { actor_role: Some(_), .. }) => true,

        // Project edit / archive / member management: project leads only,
        // and never on an archived project (un-archive first).
        (
            Member,
            A::EditProject | A::AddProjectMember | A::RemoveProjectMember
            | A::ChangeProjectMemberRole | A::ManageBoards | A::ManageColumns,
            R::Project { actor_role: Some(PR::Lead), archived: false, .. },
        ) => true,

        // Archive / unarchive: lead can flip in either direction.
        (Member, A::ArchiveProject, R::Project { actor_role: Some(PR::Lead), .. }) => true,

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admin() -> Actor { Actor { id: Uuid::now_v7(), role: Role::Admin } }
    fn member() -> Actor { Actor { id: Uuid::now_v7(), role: Role::Member } }
    fn viewer() -> Actor { Actor { id: Uuid::now_v7(), role: Role::Viewer } }

    fn project(role: Option<ProjectRole>, archived: bool) -> Resource {
        Resource::Project { id: Uuid::now_v7(), actor_role: role, archived }
    }

    #[test]
    fn admin_can_do_anything() {
        assert!(can(&admin(), Action::EditProject, project(None, true)));
        assert!(can(&admin(), Action::InviteUser, Resource::Admin));
    }

    #[test]
    fn non_member_cannot_view_project() {
        assert!(!can(&member(), Action::ViewProject, project(None, false)));
    }

    #[test]
    fn member_with_any_project_role_can_view() {
        for r in [ProjectRole::Lead, ProjectRole::Contributor, ProjectRole::Watcher] {
            assert!(can(&member(), Action::ViewProject, project(Some(r), false)));
        }
    }

    #[test]
    fn only_lead_can_edit_project() {
        assert!(can(&member(), Action::EditProject, project(Some(ProjectRole::Lead), false)));
        assert!(!can(&member(), Action::EditProject, project(Some(ProjectRole::Contributor), false)));
        assert!(!can(&member(), Action::EditProject, project(Some(ProjectRole::Watcher), false)));
    }

    #[test]
    fn archived_project_blocks_edits_even_for_lead() {
        assert!(!can(&member(), Action::EditProject, project(Some(ProjectRole::Lead), true)));
        assert!(!can(&member(), Action::ManageColumns, project(Some(ProjectRole::Lead), true)));
        // But lead can still archive/unarchive (toggle) and view.
        assert!(can(&member(), Action::ArchiveProject, project(Some(ProjectRole::Lead), true)));
        assert!(can(&member(), Action::ViewProject, project(Some(ProjectRole::Lead), true)));
    }

    #[test]
    fn viewer_cannot_create_project() {
        assert!(!can(&viewer(), Action::CreateProject, Resource::SelfRef));
    }

    #[test]
    fn member_can_create_project() {
        assert!(can(&member(), Action::CreateProject, Resource::SelfRef));
    }
}
