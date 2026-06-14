//! Opt-in public read-only project status (F18). A project lead enables a page;
//! that mints a random `public_token`. Anyone with the token can read a
//! **whitelisted** summary — project name, the active sprint's progress, and
//! per-column task *counts*. Deliberately no task titles, descriptions, labels,
//! assignees, custom fields, comments, attachments, or anything vault-adjacent,
//! so a leaked token exposes only coarse progress. Disabling clears the token
//! and the URL 404s.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{AppError, AppResult};

#[derive(Debug, Serialize)]
pub struct PublicStatus {
    pub project_name: String,
    pub project_key: String,
    pub sprint: Option<PublicSprint>,
    pub columns: Vec<PublicColumn>,
}

#[derive(Debug, Serialize)]
pub struct PublicSprint {
    pub name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub total: i64,
    pub done: i64,
    pub percent: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PublicColumn {
    pub name: String,
    pub category: String,
    pub count: i64,
}

/// Enable the public page, minting a token if one doesn't exist yet (so the URL
/// is stable across repeated enables). Returns the current token.
pub async fn enable(db: &PgPool, project_id: Uuid) -> AppResult<String> {
    let token = random_token();
    let current: Option<String> = sqlx::query_scalar(
        r#"UPDATE projects
              SET public_token = COALESCE(public_token, $2), updated_at = now()
            WHERE id = $1 AND deleted_at IS NULL
          RETURNING public_token"#,
    )
    .bind(project_id)
    .bind(&token)
    .fetch_optional(db)
    .await?
    .flatten();
    current.ok_or(AppError::NotFound)
}

pub async fn disable(db: &PgPool, project_id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE projects SET public_token = NULL, updated_at = now() WHERE id = $1")
        .bind(project_id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn current_token(db: &PgPool, project_id: Uuid) -> AppResult<Option<String>> {
    Ok(
        sqlx::query_scalar(
            "SELECT public_token FROM projects WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .fetch_optional(db)
        .await?
        .flatten(),
    )
}

/// Resolve a public token to its whitelisted status summary. `NotFound` for an
/// unknown or disabled token.
pub async fn load_by_token(db: &PgPool, token: &str) -> AppResult<PublicStatus> {
    let (project_id, project_key, project_name): (Uuid, String, String) = sqlx::query_as(
        r#"SELECT id, key, name FROM projects
            WHERE public_token = $1 AND deleted_at IS NULL"#,
    )
    .bind(token)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    // Active sprint progress (if any).
    let sprint_row: Option<(Uuid, String, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT id, name, starts_at, ends_at FROM sprints
            WHERE project_id = $1 AND state = 'active' AND deleted_at IS NULL
            ORDER BY starts_at DESC LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(db)
    .await?;

    let sprint = match sprint_row {
        Some((sprint_id, name, starts_at, ends_at)) => {
            let (total, done): (i64, i64) = sqlx::query_as(
                r#"SELECT COUNT(*),
                          COUNT(*) FILTER (WHERE status = 'done')
                     FROM tasks
                    WHERE sprint_id = $1 AND deleted_at IS NULL"#,
            )
            .bind(sprint_id)
            .fetch_one(db)
            .await?;
            let percent = if total > 0 { done * 100 / total } else { 0 };
            Some(PublicSprint {
                name,
                starts_at,
                ends_at,
                total,
                done,
                percent,
            })
        }
        None => None,
    };

    // Default board's columns + task counts.
    let columns = sqlx::query_as::<_, PublicColumn>(
        r#"SELECT bc.name, bc.category, COUNT(t.id) AS count
             FROM boards b
             JOIN board_columns bc ON bc.board_id = b.id AND bc.deleted_at IS NULL
             LEFT JOIN tasks t ON t.column_id = bc.id AND t.deleted_at IS NULL
            WHERE b.project_id = $1 AND b.deleted_at IS NULL
              AND b.id = (
                  SELECT id FROM boards
                   WHERE project_id = $1 AND deleted_at IS NULL
                   ORDER BY is_default DESC, created_at
                   LIMIT 1
              )
            GROUP BY bc.id, bc.name, bc.category, bc.sort_order
            ORDER BY bc.sort_order"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    Ok(PublicStatus {
        project_name,
        project_key,
        sprint,
        columns,
    })
}

fn random_token() -> String {
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
