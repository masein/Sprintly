//! Time log endpoints. Per-task and per-user views, plus the global timer.
//!
//!   POST   /tasks/:key/timer/start         — start a fresh running log on this task.
//!                                            409 if another log is already running for you.
//!   POST   /timer/stop                     — close your running log (idempotent if none).
//!   GET    /me/timer                       — current running log or null.
//!
//!   POST   /tasks/:key/time-logs           — manual entry (started_at + duration).
//!   GET    /tasks/:key/time-logs           — list for a task.
//!   PATCH  /time-logs/:id                  — edit (note, billable, duration); blocked
//!                                            if the parent week is approved.
//!   DELETE /time-logs/:id                  — soft; same approval lock.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action, Role as GlobalRole},
        projects as project_ctx, tasks as task_domain, timesheets as ts,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tasks/:task_key/timer/start", post(start_timer))
        .route("/timer/stop", post(stop_timer))
        .route("/me/timer", get(current_timer))
        .route(
            "/tasks/:task_key/time-logs",
            post(create_manual).get(list_for_task),
        )
        .route(
            "/time-logs/:id",
            axum::routing::patch(edit_log).delete(delete_log),
        )
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TimeLogDto {
    pub id: Uuid,
    pub task_id: Uuid,
    pub task_key: String,
    pub project_key: String,
    pub user_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_minutes: Option<i32>,
    pub note: String,
    pub billable: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateManualReq {
    pub started_at: DateTime<Utc>,
    /// minutes; must be > 0
    pub duration_minutes: i32,
    #[validate(length(max = 1000))]
    pub note: Option<String>,
    pub billable: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditLogReq {
    #[validate(length(max = 1000))]
    pub note: Option<String>,
    pub billable: Option<bool>,
    /// minutes; if provided, sets ended_at = started_at + N min
    pub duration_minutes: Option<i32>,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn start_timer(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    // Resolve task → project → access check.
    let task = sqlx::query!(
        r#"
        SELECT t.id        AS "id!: Uuid",
               t.project_id AS "project_id!: Uuid"
        FROM   tasks t
        WHERE  t.key = $1 AND t.deleted_at IS NULL
        "#,
        task_key
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let id = Uuid::now_v7();
    let insert = sqlx::query(
        r#"
        INSERT INTO time_logs (id, task_id, user_id, started_at)
        VALUES ($1, $2, $3, now())
        "#,
    )
    .bind(id)
    .bind(task.id)
    .bind(user.id)
    .execute(&state.db)
    .await;

    if let Err(sqlx::Error::Database(db)) = &insert {
        if db.is_unique_violation() {
            return Err(AppError::Conflict(
                "you already have a running timer — stop it first".into(),
            ));
        }
    }
    insert?;

    // Activity row.
    let mut tx = state.db.begin().await?;
    task_domain::log_activity(
        &mut tx,
        task.id,
        Some(user.id),
        "time_logged",
        &serde_json::json!({ "log_id": id, "action": "started" }),
    )
    .await?;
    tx.commit().await?;

    let dto = fetch_log(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn stop_timer(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    // Close whichever running log this user has, if any. Idempotent.
    let row = sqlx::query!(
        r#"
        UPDATE time_logs
           SET ended_at = now()
         WHERE user_id = $1 AND ended_at IS NULL AND deleted_at IS NULL
        RETURNING id        AS "id!: Uuid",
                  task_id   AS "task_id!: Uuid"
        "#,
        user.id
    )
    .fetch_optional(&state.db)
    .await?;

    if let Some(row) = row {
        let mut tx = state.db.begin().await?;
        task_domain::log_activity(
            &mut tx,
            row.task_id,
            Some(user.id),
            "time_logged",
            &serde_json::json!({ "log_id": row.id, "action": "stopped" }),
        )
        .await?;
        tx.commit().await?;
        let dto = fetch_log(&state.db, row.id).await?;
        return Ok((StatusCode::OK, Json(serde_json::json!({ "stopped": dto }))));
    }
    Ok((StatusCode::OK, Json(serde_json::json!({ "stopped": null }))))
}

async fn current_timer(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let row: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM time_logs
           WHERE user_id = $1 AND ended_at IS NULL AND deleted_at IS NULL"#,
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(match row {
        Some(id) => serde_json::json!({ "running": fetch_log(&state.db, id).await? }),
        None => serde_json::json!({ "running": null }),
    }))
}

async fn create_manual(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<CreateManualReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if req.duration_minutes <= 0 {
        return Err(AppError::Validation("duration_minutes must be > 0".into()));
    }
    let task = sqlx::query!(
        r#"
        SELECT id         AS "id!: Uuid",
               project_id AS "project_id!: Uuid"
        FROM   tasks WHERE key = $1 AND deleted_at IS NULL
        "#,
        task_key
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    ensure_week_open(&state.db, user.id, req.started_at).await?;

    let ended_at = req.started_at + Duration::minutes(req.duration_minutes as i64);
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, note, billable)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(task.id)
    .bind(user.id)
    .bind(req.started_at)
    .bind(ended_at)
    .bind(req.note.as_deref().unwrap_or(""))
    .bind(req.billable.unwrap_or(true))
    .execute(&state.db)
    .await?;

    let mut tx = state.db.begin().await?;
    task_domain::log_activity(
        &mut tx,
        task.id,
        Some(user.id),
        "time_logged",
        &serde_json::json!({ "log_id": id, "action": "manual", "minutes": req.duration_minutes }),
    )
    .await?;
    tx.commit().await?;

    let dto = fetch_log(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn list_for_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = sqlx::query!(
        r#"
        SELECT id         AS "id!: Uuid",
               project_id AS "project_id!: Uuid"
        FROM   tasks WHERE key = $1 AND deleted_at IS NULL
        "#,
        task_key
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT tl.id              AS "id!: Uuid",
               tl.task_id         AS "task_id!: Uuid",
               t.key              AS "task_key!: String",
               p.key              AS "project_key!: String",
               tl.user_id         AS "user_id!: Uuid",
               tl.started_at      AS "started_at!: DateTime<Utc>",
               tl.ended_at,
               tl.duration_minutes,
               tl.note            AS "note!: String",
               tl.billable        AS "billable!: bool"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        JOIN   projects p ON p.id = t.project_id
        WHERE  tl.task_id = $1 AND tl.deleted_at IS NULL
        ORDER  BY tl.started_at DESC
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<TimeLogDto> = rows
        .into_iter()
        .map(|r| TimeLogDto {
            id: r.id,
            task_id: r.task_id,
            task_key: r.task_key,
            project_key: r.project_key,
            user_id: r.user_id,
            started_at: r.started_at,
            ended_at: r.ended_at,
            duration_minutes: r.duration_minutes,
            note: r.note,
            billable: r.billable,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn edit_log(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<EditLogReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let row = sqlx::query!(
        r#"
        SELECT user_id    AS "user_id!: Uuid",
               started_at AS "started_at!: DateTime<Utc>",
               ended_at,
               task_id    AS "task_id!: Uuid"
        FROM   time_logs WHERE id = $1 AND deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    // Only the owner edits; admins also allowed.
    if row.user_id != user.id && user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    ensure_week_open(&state.db, row.user_id, row.started_at).await?;

    let new_ended_at = req
        .duration_minutes
        .map(|m| row.started_at + Duration::minutes(m as i64));

    sqlx::query(
        r#"
        UPDATE time_logs SET
            note     = COALESCE($2, note),
            billable = COALESCE($3, billable),
            ended_at = COALESCE($4, ended_at)
         WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(req.note)
    .bind(req.billable)
    .bind(new_ended_at)
    .execute(&state.db)
    .await?;

    let dto = fetch_log(&state.db, id).await?;
    Ok(Json(dto))
}

async fn delete_log(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let row = sqlx::query!(
        r#"
        SELECT user_id    AS "user_id!: Uuid",
               started_at AS "started_at!: DateTime<Utc>"
        FROM   time_logs WHERE id = $1 AND deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.user_id != user.id && user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    ensure_week_open(&state.db, row.user_id, row.started_at).await?;
    sqlx::query("UPDATE time_logs SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── helpers ────────────────────────────────────────────────────────────────

async fn fetch_log(db: &PgPool, id: Uuid) -> AppResult<TimeLogDto> {
    let r = sqlx::query!(
        r#"
        SELECT tl.id              AS "id!: Uuid",
               tl.task_id         AS "task_id!: Uuid",
               t.key              AS "task_key!: String",
               p.key              AS "project_key!: String",
               tl.user_id         AS "user_id!: Uuid",
               tl.started_at      AS "started_at!: DateTime<Utc>",
               tl.ended_at,
               tl.duration_minutes,
               tl.note            AS "note!: String",
               tl.billable        AS "billable!: bool"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        JOIN   projects p ON p.id = t.project_id
        WHERE  tl.id = $1
        "#,
        id
    )
    .fetch_one(db)
    .await?;
    Ok(TimeLogDto {
        id: r.id,
        task_id: r.task_id,
        task_key: r.task_key,
        project_key: r.project_key,
        user_id: r.user_id,
        started_at: r.started_at,
        ended_at: r.ended_at,
        duration_minutes: r.duration_minutes,
        note: r.note,
        billable: r.billable,
    })
}

/// Reject writes that would mutate a time log inside an approved week.
async fn ensure_week_open(db: &PgPool, user_id: Uuid, started_at: DateTime<Utc>) -> AppResult<()> {
    let (monday, _sunday) = ts::week_bounds(started_at.date_naive());
    let status: Option<String> = sqlx::query_scalar(
        r#"SELECT status FROM timesheets
           WHERE user_id = $1 AND period_start = $2"#,
    )
    .bind(user_id)
    .bind(monday)
    .fetch_optional(db)
    .await?;
    match status.as_deref() {
        Some("approved") | Some("paid") => Err(AppError::Conflict(
            "timesheet for this week is locked — ask an admin".into(),
        )),
        _ => Ok(()),
    }
}
