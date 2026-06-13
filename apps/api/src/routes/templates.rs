//! Task templates, the backlog, and bulk task ops (F9).
//!
//!   GET    /projects/:key/templates
//!   POST   /projects/:key/templates             { name, title, description?, type?, priority?, labels?, recurrence? }
//!   PATCH  /templates/:id
//!   DELETE /templates/:id
//!   POST   /templates/:id/instantiate           { column_id? } → { key }
//!   GET    /projects/:key/backlog               unscheduled (no-sprint) tasks
//!   POST   /projects/:key/tasks/bulk            { task_keys, op, … } → { affected }

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx, templates,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/templates", get(list).post(create))
        .route(
            "/templates/:id",
            axum::routing::patch(update).delete(remove),
        )
        .route("/templates/:id/instantiate", post(instantiate))
        .route("/projects/:key/backlog", get(backlog))
        .route("/projects/:key/tasks/bulk", post(bulk))
}

const TYPES: [&str; 5] = ["feature", "bug", "chore", "spike", "incident"];
const PRIORITIES: [&str; 4] = ["p0", "p1", "p2", "p3"];

#[derive(Debug, Deserialize)]
struct CreateReq {
    name: String,
    title: String,
    description: Option<String>,
    r#type: Option<String>,
    priority: Option<String>,
    labels: Option<Vec<String>>,
    recurrence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    r#type: Option<String>,
    priority: Option<String>,
    labels: Option<Vec<String>>,
    recurrence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstantiateReq {
    column_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum BulkOp {
    Assign { assignee_id: Option<Uuid> },
    Sprint { sprint_id: Option<Uuid> },
    Column { column_id: Uuid },
    Label { labels: Vec<String> },
    Delete,
}

#[derive(Debug, Deserialize)]
struct BulkReq {
    task_keys: Vec<String>,
    #[serde(flatten)]
    op: BulkOp,
}

fn check_name(name: &str, what: &str) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > 120 {
        return Err(AppError::BadRequest(format!("{what} must be 1–120 chars")));
    }
    Ok(())
}

fn check_enums(r#type: &str, priority: &str, recurrence: &str) -> AppResult<()> {
    if !TYPES.contains(&r#type) {
        return Err(AppError::BadRequest("type invalid".into()));
    }
    if !PRIORITIES.contains(&priority) {
        return Err(AppError::BadRequest("priority invalid".into()));
    }
    if !templates::valid_recurrence(recurrence) {
        return Err(AppError::BadRequest(format!(
            "recurrence must be one of: {}",
            templates::RECURRENCES.join(", ")
        )));
    }
    Ok(())
}

async fn editor_ctx(
    state: &AppState,
    user: &CurrentUser,
    project_key: &str,
) -> AppResult<project_ctx::ProjectContext> {
    let ctx = project_ctx::load_by_key(&state.db, project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(ctx)
}

// ─── templates ───────────────────────────────────────────────────────────────

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(templates::list(&state.db, ctx.id).await?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = editor_ctx(&state, &user, &project_key).await?;
    check_name(&req.name, "template name")?;
    check_name(&req.title, "task title")?;
    let ty = req.r#type.as_deref().unwrap_or("feature");
    let prio = req.priority.as_deref().unwrap_or("p2");
    let recurrence = req.recurrence.as_deref().unwrap_or("none");
    check_enums(ty, prio, recurrence)?;

    let next_run_at = templates::next_run(recurrence, Utc::now());
    let labels = req.labels.unwrap_or_default();
    let t = templates::create(
        &state.db,
        ctx.id,
        req.name.trim(),
        req.title.trim(),
        req.description.as_deref().unwrap_or(""),
        ty,
        prio,
        &labels,
        recurrence,
        next_run_at,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(t)))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateReq>,
) -> AppResult<impl IntoResponse> {
    let pid = templates::project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(n) = &req.name {
        check_name(n, "template name")?;
    }
    if let Some(t) = &req.title {
        check_name(t, "task title")?;
    }
    if let Some(t) = &req.r#type {
        if !TYPES.contains(&t.as_str()) {
            return Err(AppError::BadRequest("type invalid".into()));
        }
    }
    if let Some(p) = &req.priority {
        if !PRIORITIES.contains(&p.as_str()) {
            return Err(AppError::BadRequest("priority invalid".into()));
        }
    }
    // Changing the cadence resets the next run.
    let next_run_at = match req.recurrence.as_deref() {
        Some(r) => {
            if !templates::valid_recurrence(r) {
                return Err(AppError::BadRequest("recurrence invalid".into()));
            }
            templates::next_run(r, Utc::now())
        }
        None => None,
    };
    let t = templates::update(
        &state.db,
        id,
        pid,
        req.name.as_deref().map(str::trim),
        req.title.as_deref().map(str::trim),
        req.description.as_deref(),
        req.r#type.as_deref(),
        req.priority.as_deref(),
        req.labels.as_deref(),
        req.recurrence.as_deref(),
        next_run_at,
    )
    .await?;
    Ok(Json(t))
}

async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = templates::project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    templates::delete(&state.db, id, pid).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn instantiate(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<InstantiateReq>,
) -> AppResult<impl IntoResponse> {
    let t = templates::get(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, t.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let (_, key) = templates::instantiate(&state.db, &t, Some(user.id), req.column_id).await?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "key": key }))))
}

// ─── backlog ─────────────────────────────────────────────────────────────────

async fn backlog(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(templates::backlog(&state.db, ctx.id).await?))
}

// ─── bulk ────────────────────────────────────────────────────────────────────

async fn bulk(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<BulkReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = editor_ctx(&state, &user, &project_key).await?;
    if req.task_keys.is_empty() {
        return Err(AppError::BadRequest("no tasks selected".into()));
    }
    if req.task_keys.len() > 500 {
        return Err(AppError::BadRequest("too many tasks in one bulk op".into()));
    }

    let affected = match req.op {
        BulkOp::Assign { assignee_id } => {
            templates::bulk_assign(&state.db, ctx.id, &req.task_keys, assignee_id).await?
        }
        BulkOp::Sprint { sprint_id } => {
            if let Some(sid) = sprint_id {
                let ok: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM sprints WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL)",
                )
                .bind(sid)
                .bind(ctx.id)
                .fetch_one(&state.db)
                .await?;
                if !ok {
                    return Err(AppError::BadRequest("sprint not in this project".into()));
                }
            }
            templates::bulk_sprint(&state.db, ctx.id, &req.task_keys, sprint_id).await?
        }
        BulkOp::Column { column_id } => {
            let row: Option<(Uuid, String)> = sqlx::query_as(
                r#"SELECT bc.board_id, bc.category FROM board_columns bc
                   JOIN boards b ON b.id = bc.board_id
                   WHERE bc.id = $1 AND bc.deleted_at IS NULL
                     AND b.project_id = $2 AND b.deleted_at IS NULL"#,
            )
            .bind(column_id)
            .bind(ctx.id)
            .fetch_optional(&state.db)
            .await?;
            let (board_id, category) =
                row.ok_or(AppError::BadRequest("column not in this project".into()))?;
            templates::bulk_move_column(
                &state.db,
                ctx.id,
                &req.task_keys,
                column_id,
                board_id,
                &category,
            )
            .await?
        }
        BulkOp::Label { labels } => {
            templates::bulk_labels(&state.db, ctx.id, &req.task_keys, &labels).await?
        }
        BulkOp::Delete => templates::bulk_delete(&state.db, ctx.id, &req.task_keys).await?,
    };

    Ok(Json(serde_json::json!({ "affected": affected })))
}
