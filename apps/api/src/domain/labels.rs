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
    let mut tx = db.begin().await?;
    let old_name: Option<String> = sqlx::query_scalar(
        "SELECT name FROM project_labels WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let old_name = old_name.ok_or(AppError::NotFound)?;

    let label: Label = sqlx::query_as(
        r#"UPDATE project_labels SET name = COALESCE($3, name), color = COALESCE($4, color)
           WHERE id = $1 AND project_id = $2
           RETURNING id, project_id, name, color, created_at"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(color)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            AppError::Conflict("a label with that name already exists".into())
        } else {
            e.into()
        }
    })?;

    // Tasks carry labels as names, not ids, so renaming only the registry row
    // would strand every tagged task on the old name — still showing it, now
    // uncoloured, and no longer matched by `label:new`. Carry the rename into
    // the tasks in the same transaction. Matching is case-insensitive like the
    // board's colour lookup; a task that somehow had both names keeps one.
    if label.name != old_name {
        rename_on_tasks(&mut tx, project_id, &old_name, &label.name).await?;
    }
    tx.commit().await?;
    Ok(label)
}

/// Replace `old` with `new` in every task's label list in the project,
/// keeping each list's order and dropping a duplicate the rename would create.
/// `updated_at` is left alone on purpose: a palette edit isn't work on the
/// task, and bumping it would reshuffle every "recently updated" list.
async fn rename_on_tasks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    old: &str,
    new: &str,
) -> AppResult<u64> {
    let r = sqlx::query(
        r#"
        UPDATE tasks t
        SET    labels = ARRAY(
                   SELECT v FROM (
                       SELECT DISTINCT ON (lower(v)) v, i
                       FROM (
                           SELECT CASE WHEN lower(l) = lower($2) THEN $3 ELSE l END AS v, i
                           FROM   unnest(t.labels) WITH ORDINALITY AS u(l, i)
                       ) renamed
                       ORDER BY lower(v), i
                   ) deduped
                   ORDER BY i
               )
        WHERE  t.project_id = $1
          AND  EXISTS (SELECT 1 FROM unnest(t.labels) l WHERE lower(l) = lower($2))
        "#,
    )
    .bind(project_id)
    .bind(old)
    .bind(new)
    .execute(&mut **tx)
    .await?;
    Ok(r.rows_affected())
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
