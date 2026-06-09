//! Per-project label registry (name → colour).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{AppError, AppResult};

#[derive(Debug, Serialize, FromRow)]
pub struct Label {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}

/// `#rgb` or `#rrggbb`.
pub fn valid_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

pub async fn list(db: &PgPool, project_id: Uuid) -> AppResult<Vec<Label>> {
    let rows = sqlx::query_as(
        r#"SELECT id, project_id, name, color, created_at
           FROM project_labels WHERE project_id = $1 ORDER BY lower(name)"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn create(db: &PgPool, project_id: Uuid, name: &str, color: &str) -> AppResult<Label> {
    sqlx::query_as(
        r#"INSERT INTO project_labels (id, project_id, name, color)
           VALUES ($1, $2, $3, $4)
           RETURNING id, project_id, name, color, created_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(color)
    .fetch_one(db)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            AppError::Conflict("a label with that name already exists".into())
        } else {
            e.into()
        }
    })
}

pub async fn update(
    db: &PgPool,
    id: Uuid,
    project_id: Uuid,
    name: Option<&str>,
    color: Option<&str>,
) -> AppResult<Label> {
    let row: Option<Label> = sqlx::query_as(
        r#"UPDATE project_labels SET name = COALESCE($3, name), color = COALESCE($4, color)
           WHERE id = $1 AND project_id = $2
           RETURNING id, project_id, name, color, created_at"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(color)
    .fetch_optional(db)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            AppError::Conflict("a label with that name already exists".into())
        } else {
            e.into()
        }
    })?;
    row.ok_or(AppError::NotFound)
}

pub async fn delete(db: &PgPool, id: Uuid, project_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM project_labels WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// The project a label belongs to (for access checks before edit/delete).
pub async fn project_of(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar(r#"SELECT project_id FROM project_labels WHERE id = $1"#)
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_validation() {
        assert!(valid_color("#7c5cff"));
        assert!(valid_color("#abc"));
        assert!(!valid_color("7c5cff"));
        assert!(!valid_color("#xyz"));
        assert!(!valid_color("#12"));
    }
}
