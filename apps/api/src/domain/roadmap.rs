//! Roadmap (F6): epics (date-ranged, coloured task groupings) and milestones
//! (dated targets). Epic progress (done/total of its tasks) is computed at
//! read time via correlated subqueries, so it's always live.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{AppError, AppResult};

#[derive(Debug, Serialize, FromRow)]
pub struct Epic {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub color: String,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub task_count: i64,
    pub done_count: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Milestone {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub due_date: NaiveDate,
    pub created_at: DateTime<Utc>,
}

// Live progress for an epic, as two correlated subqueries usable in SELECT or
// RETURNING. `e` is the epics row alias.
const PROGRESS: &str = r#"
    (SELECT count(*) FROM tasks t
       WHERE t.epic_id = e.id AND t.deleted_at IS NULL)::int8 AS task_count,
    (SELECT count(*) FROM tasks t
       WHERE t.epic_id = e.id AND t.deleted_at IS NULL AND t.status = 'done')::int8 AS done_count
"#;
const EPIC_COLS: &str =
    "e.id, e.project_id, e.name, e.color, e.start_date, e.end_date, e.created_at";

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

// ─── epics ──────────────────────────────────────────────────────────────────

pub async fn epics_list(db: &PgPool, project_id: Uuid) -> AppResult<Vec<Epic>> {
    let rows = sqlx::query_as(&format!(
        r#"SELECT {EPIC_COLS}, {PROGRESS}
           FROM epics e WHERE e.project_id = $1
           ORDER BY e.start_date NULLS LAST, lower(e.name)"#
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn epic_create(
    db: &PgPool,
    project_id: Uuid,
    name: &str,
    color: &str,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
) -> AppResult<Epic> {
    // A new epic has no tasks yet, so its progress is a literal 0/0 — no need
    // for the correlated subqueries here.
    let row = sqlx::query_as(
        r#"INSERT INTO epics (id, project_id, name, color, start_date, end_date)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, project_id, name, color, start_date, end_date, created_at,
                     0::int8 AS task_count, 0::int8 AS done_count"#,
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(color)
    .bind(start)
    .bind(end)
    .fetch_one(db)
    .await?;
    Ok(row)
}

/// Update an epic. `name`/`color` use COALESCE (omit to keep); dates use an
/// explicit "set" flag so a date can be cleared as well as changed.
#[allow(clippy::too_many_arguments)]
pub async fn epic_update(
    db: &PgPool,
    id: Uuid,
    project_id: Uuid,
    name: Option<&str>,
    color: Option<&str>,
    set_start: bool,
    start: Option<NaiveDate>,
    set_end: bool,
    end: Option<NaiveDate>,
) -> AppResult<Epic> {
    let row: Option<Epic> = sqlx::query_as(&format!(
        r#"UPDATE epics AS e SET
               name       = COALESCE($3, e.name),
               color      = COALESCE($4, e.color),
               start_date = CASE WHEN $5 THEN $6 ELSE e.start_date END,
               end_date   = CASE WHEN $7 THEN $8 ELSE e.end_date END,
               updated_at = now()
           WHERE e.id = $1 AND e.project_id = $2
           RETURNING {EPIC_COLS}, {PROGRESS}"#
    ))
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(color)
    .bind(set_start)
    .bind(start)
    .bind(set_end)
    .bind(end)
    .fetch_optional(db)
    .await?;
    row.ok_or(AppError::NotFound)
}

pub async fn epic_delete(db: &PgPool, id: Uuid, project_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM epics WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn epic_project_of(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT project_id FROM epics WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

/// Assign (or clear, with `None`) a task's epic. The caller must have already
/// confirmed the epic — when `Some` — belongs to the task's project.
pub async fn assign_task_epic(db: &PgPool, task_id: Uuid, epic_id: Option<Uuid>) -> AppResult<()> {
    sqlx::query("UPDATE tasks SET epic_id = $2, updated_at = now() WHERE id = $1")
        .bind(task_id)
        .bind(epic_id)
        .execute(db)
        .await?;
    Ok(())
}

// ─── milestones ─────────────────────────────────────────────────────────────

pub async fn milestones_list(db: &PgPool, project_id: Uuid) -> AppResult<Vec<Milestone>> {
    let rows = sqlx::query_as(
        r#"SELECT id, project_id, name, due_date, created_at
           FROM milestones WHERE project_id = $1 ORDER BY due_date, lower(name)"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn milestone_create(
    db: &PgPool,
    project_id: Uuid,
    name: &str,
    due_date: NaiveDate,
) -> AppResult<Milestone> {
    sqlx::query_as(
        r#"INSERT INTO milestones (id, project_id, name, due_date)
           VALUES ($1, $2, $3, $4)
           RETURNING id, project_id, name, due_date, created_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(due_date)
    .fetch_one(db)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            AppError::Conflict("milestone already exists".into())
        } else {
            e.into()
        }
    })
}

pub async fn milestone_update(
    db: &PgPool,
    id: Uuid,
    project_id: Uuid,
    name: Option<&str>,
    due_date: Option<NaiveDate>,
) -> AppResult<Milestone> {
    let row: Option<Milestone> = sqlx::query_as(
        r#"UPDATE milestones SET
               name = COALESCE($3, name),
               due_date = COALESCE($4, due_date),
               updated_at = now()
           WHERE id = $1 AND project_id = $2
           RETURNING id, project_id, name, due_date, created_at"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(due_date)
    .fetch_optional(db)
    .await?;
    row.ok_or(AppError::NotFound)
}

pub async fn milestone_delete(db: &PgPool, id: Uuid, project_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM milestones WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn milestone_project_of(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT project_id FROM milestones WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}
