//! Task-shaped pure logic + the operations the route layer needs:
//!
//!   • `next_key()` — atomically increment `projects.next_task_seq` and
//!     return the formatted `PROJ-N` key. Uses `UPDATE … RETURNING` so
//!     concurrent requests can't ever collide.
//!
//!   • `position_after/before/append` — fractional ordering helpers,
//!     compatible with the board's column ordering.
//!
//!   • `log_activity()` — write a `task_activity` row inside an existing
//!     transaction. Shared by every route that mutates a task.
//!
//! These live in `domain::` because they don't need an HTTP context. Tests
//! exercise them directly.

use sqlx::PgPool;
use uuid::Uuid;

use crate::{AppError, AppResult};

/// Atomically reserve the next task key for a project. Returns the formatted
/// string (e.g. `"WEB-7"`) and the integer just consumed.
pub async fn next_key(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
) -> AppResult<(String, i64)> {
    let row = sqlx::query!(
        r#"
        UPDATE projects
           SET next_task_seq = next_task_seq + 1
         WHERE id = $1 AND deleted_at IS NULL
        RETURNING key             AS "key!: String",
                  next_task_seq - 1 AS "seq!: i64"
        "#,
        project_id
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok((format!("{}-{}", row.key, row.seq), row.seq))
}

/// Where to drop a card in a column.
#[derive(Debug, Clone, Copy)]
pub enum Position {
    Append,            // place at the end
    Prepend,           // place at the front
    Between(f64, f64), // place between two existing order values
}

/// Compute a concrete `order_in_column` given current min/max and an intent.
pub fn position_value(min: Option<f64>, max: Option<f64>, intent: Position) -> f64 {
    match intent {
        Position::Append => max.unwrap_or(0.0) + 1024.0,
        Position::Prepend => min.unwrap_or(2048.0) - 1024.0,
        Position::Between(a, b) => (a + b) / 2.0,
    }
}

/// Pick a destination position for a move:
///   • If `after_id` is given, drop after that task.
///   • Else if `before_id` is given, drop before it.
///   • Else, append.
pub async fn resolve_position(
    db: &PgPool,
    column_id: Uuid,
    after_id: Option<Uuid>,
    before_id: Option<Uuid>,
) -> AppResult<f64> {
    // Helper: read one task's order_in_column in this column.
    async fn order_in(db: &PgPool, column_id: Uuid, task_id: Uuid) -> AppResult<Option<f64>> {
        Ok(sqlx::query_scalar(
            r#"SELECT order_in_column FROM tasks
               WHERE id = $1 AND column_id = $2 AND deleted_at IS NULL"#,
        )
        .bind(task_id)
        .bind(column_id)
        .fetch_optional(db)
        .await?)
    }

    if let Some(after) = after_id {
        let after_o = order_in(db, column_id, after)
            .await?
            .ok_or(AppError::NotFound)?;
        let next_o: Option<f64> = sqlx::query_scalar(
            r#"SELECT MIN(order_in_column) FROM tasks
               WHERE column_id = $1 AND deleted_at IS NULL AND order_in_column > $2"#,
        )
        .bind(column_id)
        .bind(after_o)
        .fetch_one(db)
        .await?;
        return Ok(match next_o {
            Some(n) => position_value(None, None, Position::Between(after_o, n)),
            None => after_o + 1024.0,
        });
    }

    if let Some(before) = before_id {
        let before_o = order_in(db, column_id, before)
            .await?
            .ok_or(AppError::NotFound)?;
        let prev_o: Option<f64> = sqlx::query_scalar(
            r#"SELECT MAX(order_in_column) FROM tasks
               WHERE column_id = $1 AND deleted_at IS NULL AND order_in_column < $2"#,
        )
        .bind(column_id)
        .bind(before_o)
        .fetch_one(db)
        .await?;
        return Ok(match prev_o {
            Some(p) => position_value(None, None, Position::Between(p, before_o)),
            None => before_o - 1024.0,
        });
    }

    // Append.
    let max_o: Option<f64> = sqlx::query_scalar(
        r#"SELECT MAX(order_in_column) FROM tasks
           WHERE column_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(column_id)
    .fetch_one(db)
    .await?;
    Ok(position_value(None, max_o, Position::Append))
}

/// Insert an activity row inside the caller's transaction.
pub async fn log_activity(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: Uuid,
    actor_id: Option<Uuid>,
    kind: &str,
    payload: &serde_json::Value,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO task_activity (id, task_id, actor_id, kind, payload)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(task_id)
    .bind(actor_id)
    .bind(kind)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_into_empty_column() {
        let v = position_value(None, None, Position::Append);
        assert!(v > 0.0);
    }

    #[test]
    fn between_strictly_between() {
        let v = position_value(None, None, Position::Between(1.0, 2.0));
        assert!(v > 1.0 && v < 2.0);
    }

    #[test]
    fn prepend_into_empty_column() {
        let v = position_value(None, None, Position::Prepend);
        // The value is "in front of nothing" — we picked a sentinel. Just
        // assert it's a finite number; the move endpoint normalises later.
        assert!(v.is_finite());
    }
}
