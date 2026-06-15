//! Task templates + recurrence (F9), plus backlog read and bulk task ops.
//!
//! A template is a task skeleton. With a `recurrence` other than `none` the
//! background worker materialises a task each interval (`materialise_due`,
//! which takes `now` so tests can advance the clock). `instantiate` makes one
//! task from a template on demand. Bulk ops + the backlog query round out the
//! "manage the backlog efficiently" half.

use chrono::{DateTime, Duration, Months, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{domain::tasks as task_domain, AppError, AppResult};

pub const RECURRENCES: [&str; 4] = ["none", "daily", "weekly", "monthly"];

pub fn valid_recurrence(r: &str) -> bool {
    RECURRENCES.contains(&r)
}

/// Next occurrence strictly after `from` for a recurrence rule. `None` for
/// `none` (or an unknown rule). Pure — unit-tested.
pub fn next_run(recurrence: &str, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match recurrence {
        "daily" => Some(from + Duration::days(1)),
        "weekly" => Some(from + Duration::weeks(1)),
        // Month arithmetic clamps (Jan 31 + 1mo → Feb 28/29).
        "monthly" => from.checked_add_months(Months::new(1)),
        _ => None,
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct Template {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub title: String,
    pub description: String,
    pub r#type: String,
    pub priority: String,
    pub labels: Vec<String>,
    pub recurrence: String,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const COLS: &str =
    "id, project_id, name, title, description, type, priority, labels, recurrence, next_run_at, created_at";

// ─── template CRUD ───────────────────────────────────────────────────────────

pub async fn list(db: &PgPool, project_id: Uuid) -> AppResult<Vec<Template>> {
    let rows = sqlx::query_as(&format!(
        "SELECT {COLS} FROM task_templates WHERE project_id = $1 ORDER BY lower(name)"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn get(db: &PgPool, id: Uuid) -> AppResult<Template> {
    sqlx::query_as(&format!("SELECT {COLS} FROM task_templates WHERE id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn project_of(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT project_id FROM task_templates WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    db: &PgPool,
    project_id: Uuid,
    name: &str,
    title: &str,
    description: &str,
    r#type: &str,
    priority: &str,
    labels: &[String],
    recurrence: &str,
    next_run_at: Option<DateTime<Utc>>,
) -> AppResult<Template> {
    let row = sqlx::query_as(&format!(
        r#"INSERT INTO task_templates
               (id, project_id, name, title, description, type, priority, labels, recurrence, next_run_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           RETURNING {COLS}"#
    ))
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(title)
    .bind(description)
    .bind(r#type)
    .bind(priority)
    .bind(labels)
    .bind(recurrence)
    .bind(next_run_at)
    .fetch_one(db)
    .await?;
    Ok(row)
}

/// Update mutable fields (COALESCE keeps omitted ones). `recurrence` and
/// `next_run_at` are set together by the route when the cadence changes.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    db: &PgPool,
    id: Uuid,
    project_id: Uuid,
    name: Option<&str>,
    title: Option<&str>,
    description: Option<&str>,
    r#type: Option<&str>,
    priority: Option<&str>,
    labels: Option<&[String]>,
    recurrence: Option<&str>,
    next_run_at: Option<DateTime<Utc>>,
) -> AppResult<Template> {
    let row: Option<Template> = sqlx::query_as(&format!(
        r#"UPDATE task_templates SET
               name = COALESCE($3, name),
               title = COALESCE($4, title),
               description = COALESCE($5, description),
               type = COALESCE($6, type),
               priority = COALESCE($7, priority),
               labels = COALESCE($8, labels),
               recurrence = COALESCE($9, recurrence),
               next_run_at = CASE WHEN $9 IS NOT NULL THEN $10 ELSE next_run_at END,
               updated_at = now()
           WHERE id = $1 AND project_id = $2
           RETURNING {COLS}"#
    ))
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(title)
    .bind(description)
    .bind(r#type)
    .bind(priority)
    .bind(labels)
    .bind(recurrence)
    .bind(next_run_at)
    .fetch_optional(db)
    .await?;
    row.ok_or(AppError::NotFound)
}

pub async fn delete(db: &PgPool, id: Uuid, project_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM task_templates WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── task instantiation ──────────────────────────────────────────────────────

/// Insert a task from explicit fields into `column_id` (or the default board's
/// first column). Shared by manual "new task from template" and the worker.
/// Returns the new (task_id, key). `actor` is the creating user, if any.
#[allow(clippy::too_many_arguments)]
pub async fn instantiate_task(
    db: &PgPool,
    project_id: Uuid,
    actor: Option<Uuid>,
    title: &str,
    description: &str,
    r#type: &str,
    priority: &str,
    labels: &[String],
    column_id: Option<Uuid>,
) -> AppResult<(Uuid, String)> {
    let mut tx = db.begin().await?;

    // Resolve destination board + column.
    let (board_id, column_id): (Uuid, Uuid) = match column_id {
        Some(col) => {
            let board: Option<Uuid> = sqlx::query_scalar(
                r#"SELECT bc.board_id FROM board_columns bc
                   JOIN boards b ON b.id = bc.board_id
                   WHERE bc.id = $1 AND bc.deleted_at IS NULL
                     AND b.project_id = $2 AND b.deleted_at IS NULL"#,
            )
            .bind(col)
            .bind(project_id)
            .fetch_optional(&mut *tx)
            .await?;
            (
                board.ok_or(AppError::BadRequest(
                    "column does not belong to this project".into(),
                ))?,
                col,
            )
        }
        None => sqlx::query_as(
            r#"SELECT b.id, bc.id FROM boards b
               JOIN board_columns bc ON bc.board_id = b.id AND bc.deleted_at IS NULL
               WHERE b.project_id = $1 AND b.is_default = true AND b.deleted_at IS NULL
               ORDER BY bc.sort_order ASC LIMIT 1"#,
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::Conflict(
            "project has no default board with columns".into(),
        ))?,
    };

    let category: String = sqlx::query_scalar("SELECT category FROM board_columns WHERE id = $1")
        .bind(column_id)
        .fetch_one(&mut *tx)
        .await?;
    let max_o: Option<f64> = sqlx::query_scalar(
        "SELECT MAX(order_in_column) FROM tasks WHERE column_id = $1 AND deleted_at IS NULL",
    )
    .bind(column_id)
    .fetch_one(&mut *tx)
    .await?;
    let order_in_column = max_o.unwrap_or(0.0) + 1024.0;

    let (key, _seq) = task_domain::next_key(&mut tx, project_id).await?;
    let task_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks
               (id, project_id, board_id, column_id, key, title, description,
                type, priority, status, reporter_id, labels, order_in_column)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"#,
    )
    .bind(task_id)
    .bind(project_id)
    .bind(board_id)
    .bind(column_id)
    .bind(&key)
    .bind(title)
    .bind(description)
    .bind(r#type)
    .bind(priority)
    .bind(&category)
    .bind(actor)
    .bind(labels)
    .bind(order_in_column)
    .execute(&mut *tx)
    .await?;

    task_domain::log_activity(
        &mut tx,
        task_id,
        actor,
        "created",
        &serde_json::json!({ "title": title }),
    )
    .await?;
    tx.commit().await?;
    Ok((task_id, key))
}

/// Make one task from a template now (manual). `actor` is the requesting user.
pub async fn instantiate(
    db: &PgPool,
    template: &Template,
    actor: Option<Uuid>,
    column_id: Option<Uuid>,
) -> AppResult<(Uuid, String)> {
    instantiate_task(
        db,
        template.project_id,
        actor,
        &template.title,
        &template.description,
        &template.r#type,
        &template.priority,
        &template.labels,
        column_id,
    )
    .await
}

/// One newly-materialised task.
#[derive(Debug)]
pub struct Materialised {
    pub template_id: Uuid,
    pub task_key: String,
}

/// Materialise every recurring template due at or before `now`: spawn one task
/// and advance `next_run_at` to the next occurrence after `now` (so a template
/// that's fallen behind catches up without a burst). `now` is injected so
/// tests can fast-forward.
pub async fn materialise_due(db: &PgPool, now: DateTime<Utc>) -> AppResult<Vec<Materialised>> {
    let due: Vec<Template> = sqlx::query_as(&format!(
        r#"SELECT {COLS} FROM task_templates
           WHERE recurrence <> 'none' AND next_run_at IS NOT NULL AND next_run_at <= $1
           ORDER BY next_run_at"#
    ))
    .bind(now)
    .fetch_all(db)
    .await?;

    let mut out = Vec::with_capacity(due.len());
    for t in &due {
        let (_, key) = instantiate(db, t, None, None).await?;
        let next = next_run(&t.recurrence, now);
        sqlx::query("UPDATE task_templates SET next_run_at = $2, updated_at = now() WHERE id = $1")
            .bind(t.id)
            .bind(next)
            .execute(db)
            .await?;
        out.push(Materialised {
            template_id: t.id,
            task_key: key,
        });
    }
    Ok(out)
}

// ─── backlog ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct BacklogItem {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    pub priority: String,
    pub status: String,
    pub assignee_id: Option<Uuid>,
    pub labels: Vec<String>,
}

/// Unscheduled work: live tasks with no sprint, not yet done.
pub async fn backlog(db: &PgPool, project_id: Uuid) -> AppResult<Vec<BacklogItem>> {
    let rows = sqlx::query_as(
        r#"SELECT id, key, title, priority, status, assignee_id, labels
           FROM tasks
           WHERE project_id = $1 AND sprint_id IS NULL AND deleted_at IS NULL
             AND status <> 'done'
             -- Subtasks belong to their parent, not the top-level backlog.
             AND parent_task_id IS NULL
           ORDER BY priority, created_at"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

// ─── bulk ops ────────────────────────────────────────────────────────────────

pub async fn bulk_assign(
    db: &PgPool,
    project_id: Uuid,
    keys: &[String],
    assignee_id: Option<Uuid>,
) -> AppResult<u64> {
    let r = sqlx::query(
        r#"UPDATE tasks SET assignee_id = $3, updated_at = now()
           WHERE project_id = $1 AND key = ANY($2) AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(keys)
    .bind(assignee_id)
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}

pub async fn bulk_sprint(
    db: &PgPool,
    project_id: Uuid,
    keys: &[String],
    sprint_id: Option<Uuid>,
) -> AppResult<u64> {
    let r = sqlx::query(
        r#"UPDATE tasks SET sprint_id = $3, updated_at = now()
           WHERE project_id = $1 AND key = ANY($2) AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(keys)
    .bind(sprint_id)
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}

pub async fn bulk_labels(
    db: &PgPool,
    project_id: Uuid,
    keys: &[String],
    labels: &[String],
) -> AppResult<u64> {
    let r = sqlx::query(
        r#"UPDATE tasks SET labels = $3, updated_at = now()
           WHERE project_id = $1 AND key = ANY($2) AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(keys)
    .bind(labels)
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}

pub async fn bulk_delete(db: &PgPool, project_id: Uuid, keys: &[String]) -> AppResult<u64> {
    let r = sqlx::query(
        r#"UPDATE tasks SET deleted_at = now()
           WHERE project_id = $1 AND key = ANY($2) AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(keys)
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}

/// Move selected tasks to a column (the route resolves `board_id` + `category`
/// for it). Appends them to the column in their existing relative order.
pub async fn bulk_move_column(
    db: &PgPool,
    project_id: Uuid,
    keys: &[String],
    column_id: Uuid,
    board_id: Uuid,
    category: &str,
) -> AppResult<u64> {
    let r = sqlx::query(
        r#"
        WITH base AS (
            SELECT COALESCE(MAX(order_in_column), 0) AS m
            FROM tasks WHERE column_id = $3 AND deleted_at IS NULL
        ),
        numbered AS (
            SELECT id, row_number() OVER (ORDER BY order_in_column) AS rn
            FROM tasks
            WHERE project_id = $1 AND key = ANY($2) AND deleted_at IS NULL
        )
        UPDATE tasks t SET
            column_id = $3,
            board_id = $4,
            status = $5,
            order_in_column = (SELECT m FROM base) + numbered.rn * 1024,
            completed_at = CASE WHEN $5 = 'done' AND t.completed_at IS NULL THEN now()
                                WHEN $5 <> 'done' THEN NULL ELSE t.completed_at END,
            updated_at = now()
        FROM numbered
        WHERE t.id = numbered.id
        "#,
    )
    .bind(project_id)
    .bind(keys)
    .bind(column_id)
    .bind(board_id)
    .bind(category)
    .execute(db)
    .await?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 9, 0, 0).unwrap()
    }

    #[test]
    fn recurrence_validation() {
        for r in RECURRENCES {
            assert!(valid_recurrence(r));
        }
        assert!(!valid_recurrence("yearly"));
    }

    #[test]
    fn next_run_steps_each_cadence() {
        let from = at(2026, 1, 15);
        assert_eq!(next_run("daily", from), Some(at(2026, 1, 16)));
        assert_eq!(next_run("weekly", from), Some(at(2026, 1, 22)));
        assert_eq!(next_run("monthly", from), Some(at(2026, 2, 15)));
        assert_eq!(next_run("none", from), None);
    }

    #[test]
    fn monthly_clamps_to_short_months() {
        // Jan 31 + 1 month → Feb 28 (2026 is not a leap year).
        let jan31 = at(2026, 1, 31);
        assert_eq!(next_run("monthly", jan31), Some(at(2026, 2, 28)));
    }
}
