//! Shared project-context loader used by every project-scoped endpoint.
//!
//! Loading the project row + the actor's membership in one query keeps the
//! handler-level code uniform: every route either gets a `ProjectContext`
//! or bails with 404 / 403, before doing anything resource-specific.

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::permissions::{ProjectRole, Resource},
    AppError, AppResult,
};

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub id: Uuid,
    pub key: String,
    pub archived: bool,
    pub actor_role: Option<ProjectRole>,
}

impl ProjectContext {
    /// Convenience for handing straight to `can()`.
    pub fn as_resource(&self) -> Resource {
        Resource::Project {
            id: self.id,
            actor_role: self.actor_role,
            archived: self.archived,
        }
    }
}

/// Look up the project by its `key` and join the actor's membership.
pub async fn load_by_key(
    db: &PgPool,
    project_key: &str,
    actor_id: Uuid,
) -> AppResult<ProjectContext> {
    let row = sqlx::query!(
        r#"
        SELECT  p.id          AS "id!: Uuid",
                p.key         AS "key!: String",
                p.archived_at,
                pm.role       AS "actor_role?: String"
        FROM    projects p
        LEFT JOIN project_members pm
               ON pm.project_id = p.id AND pm.user_id = $2
        WHERE   p.key = $1 AND p.deleted_at IS NULL
        "#,
        project_key,
        actor_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(ProjectContext {
        id: row.id,
        key: row.key,
        archived: row.archived_at.is_some(),
        actor_role: row.actor_role.as_deref().and_then(ProjectRole::parse),
    })
}

/// Same, but by UUID — used for endpoints that already hold an id (boards,
/// columns) and need to reach their project context.
pub async fn load_by_id(
    db: &PgPool,
    project_id: Uuid,
    actor_id: Uuid,
) -> AppResult<ProjectContext> {
    let row = sqlx::query!(
        r#"
        SELECT  p.id          AS "id!: Uuid",
                p.key         AS "key!: String",
                p.archived_at,
                pm.role       AS "actor_role?: String"
        FROM    projects p
        LEFT JOIN project_members pm
               ON pm.project_id = p.id AND pm.user_id = $2
        WHERE   p.id = $1 AND p.deleted_at IS NULL
        "#,
        project_id,
        actor_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(ProjectContext {
        id: row.id,
        key: row.key,
        archived: row.archived_at.is_some(),
        actor_role: row.actor_role.as_deref().and_then(ProjectRole::parse),
    })
}
