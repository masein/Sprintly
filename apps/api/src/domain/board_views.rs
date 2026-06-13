//! Saved board views (F8): a named filter + swimlane grouping, private to its
//! owner or shared with the project. The `filter` payload is opaque JSON the
//! client owns; the backend only stores and scopes it.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{AppError, AppResult};

pub const GROUP_BYS: [&str; 4] = ["none", "assignee", "label", "priority"];

#[derive(Debug, Serialize, FromRow)]
pub struct BoardView {
    pub id: Uuid,
    pub project_id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub filter: Value,
    pub group_by: String,
    pub shared: bool,
    pub created_at: DateTime<Utc>,
    /// Whether the caller owns this view (only the owner may edit/delete).
    pub is_mine: bool,
}

pub fn valid_group_by(g: &str) -> bool {
    GROUP_BYS.contains(&g)
}

// Shared column list; `is_mine` is spliced per query (a comparison for list,
// a literal `true` for owner-scoped create/update).
const BASE_COLS: &str = "id, project_id, owner_id, name, filter, group_by, shared, created_at";

/// Views in the project the caller can see: their own (private + shared) plus
/// everyone else's shared ones.
pub async fn list(db: &PgPool, project_id: Uuid, caller: Uuid) -> AppResult<Vec<BoardView>> {
    let rows = sqlx::query_as(&format!(
        r#"SELECT {BASE_COLS}, (owner_id = $2) AS is_mine
           FROM board_views
           WHERE project_id = $1 AND (owner_id = $2 OR shared = true)
           ORDER BY lower(name)"#
    ))
    .bind(project_id)
    .bind(caller)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn create(
    db: &PgPool,
    project_id: Uuid,
    owner_id: Uuid,
    name: &str,
    filter: &Value,
    group_by: &str,
    shared: bool,
) -> AppResult<BoardView> {
    let row = sqlx::query_as(&format!(
        r#"INSERT INTO board_views (id, project_id, owner_id, name, filter, group_by, shared)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING {BASE_COLS}, true AS is_mine"#
    ))
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(owner_id)
    .bind(name)
    .bind(filter)
    .bind(group_by)
    .bind(shared)
    .fetch_one(db)
    .await?;
    Ok(row)
}

/// Update mutable fields. Owner-scoped: a non-owner (or missing id) gets
/// `NotFound`, which doesn't leak whether the view exists.
pub async fn update(
    db: &PgPool,
    id: Uuid,
    owner_id: Uuid,
    name: Option<&str>,
    filter: Option<&Value>,
    group_by: Option<&str>,
    shared: Option<bool>,
) -> AppResult<BoardView> {
    let row: Option<BoardView> = sqlx::query_as(&format!(
        r#"UPDATE board_views SET
               name     = COALESCE($3::text, name),
               filter   = COALESCE($4::jsonb, filter),
               group_by = COALESCE($5::text, group_by),
               shared   = COALESCE($6::bool, shared),
               updated_at = now()
           WHERE id = $1 AND owner_id = $2
           RETURNING {BASE_COLS}, true AS is_mine"#
    ))
    .bind(id)
    .bind(owner_id)
    .bind(name)
    .bind(filter)
    .bind(group_by)
    .bind(shared)
    .fetch_optional(db)
    .await?;
    row.ok_or(AppError::NotFound)
}

/// Delete a view. Owner-scoped — `NotFound` for a non-owner or missing id.
pub async fn delete(db: &PgPool, id: Uuid, owner_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM board_views WHERE id = $1 AND owner_id = $2")
        .bind(id)
        .bind(owner_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_by_validation() {
        for g in GROUP_BYS {
            assert!(valid_group_by(g));
        }
        assert!(!valid_group_by("status"));
        assert!(!valid_group_by(""));
    }
}
