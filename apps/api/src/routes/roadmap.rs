//! Roadmap (F6): epics + milestones + task→epic assignment.
//!
//!   GET    /projects/:key/epics
//!   POST   /projects/:key/epics            { name, color?, start_date?, end_date? }
//!   PATCH  /epics/:id                       { name?, color?, start_date?, end_date? }
//!   DELETE /epics/:id
//!   GET    /projects/:key/milestones
//!   POST   /projects/:key/milestones        { name, due_date }
//!   PATCH  /milestones/:id                  { name?, due_date? }
//!   DELETE /milestones/:id
//!   PUT    /tasks/:task_key/epic            { epic_id: uuid | null }

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::NaiveDate;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        labels::valid_color,
        permissions::{can, Action},
        projects as project_ctx, roadmap,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

use super::tasks::resolve_project_from_task_key;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/epics", get(list_epics).post(create_epic))
        .route(
            "/epics/:id",
            axum::routing::patch(update_epic).delete(delete_epic),
        )
        .route(
            "/projects/:key/milestones",
            get(list_milestones).post(create_milestone),
        )
        .route(
            "/milestones/:id",
            axum::routing::patch(update_milestone).delete(delete_milestone),
        )
        .route("/tasks/:task_key/epic", axum::routing::put(assign_epic))
}

/// Distinguish "field absent" from "field: null" for PATCH date semantics.
mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Ok(Some(Option::<T>::deserialize(d)?))
    }
}

#[derive(Debug, Deserialize)]
struct CreateEpicReq {
    name: String,
    color: Option<String>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct UpdateEpicReq {
    name: Option<String>,
    color: Option<String>,
    #[serde(default, with = "double_option")]
    start_date: Option<Option<NaiveDate>>,
    #[serde(default, with = "double_option")]
    end_date: Option<Option<NaiveDate>>,
}

#[derive(Debug, Deserialize)]
struct CreateMilestoneReq {
    name: String,
    due_date: NaiveDate,
}

#[derive(Debug, Deserialize)]
struct UpdateMilestoneReq {
    name: Option<String>,
    due_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct AssignEpicReq {
    epic_id: Option<Uuid>,
}

fn check_name(name: &str, what: &str) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > 80 {
        return Err(AppError::BadRequest(format!(
            "{what} name must be 1–80 chars"
        )));
    }
    Ok(())
}

fn check_color(color: &str) -> AppResult<()> {
    if valid_color(color) {
        Ok(())
    } else {
        Err(AppError::BadRequest("color must be #rgb or #rrggbb".into()))
    }
}

/// If both dates are present, end must not precede start.
fn check_range(start: Option<NaiveDate>, end: Option<NaiveDate>) -> AppResult<()> {
    if let (Some(s), Some(e)) = (start, end) {
        if e < s {
            return Err(AppError::BadRequest("end_date is before start_date".into()));
        }
    }
    Ok(())
}

// ─── epics ──────────────────────────────────────────────────────────────────

async fn list_epics(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(roadmap::epics_list(&state.db, ctx.id).await?))
}

async fn create_epic(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateEpicReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    check_name(&req.name, "epic")?;
    let color = req.color.as_deref().unwrap_or("#7c5cff");
    check_color(color)?;
    check_range(req.start_date, req.end_date)?;
    let epic = roadmap::epic_create(
        &state.db,
        ctx.id,
        req.name.trim(),
        color,
        req.start_date,
        req.end_date,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(epic)))
}

async fn update_epic(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateEpicReq>,
) -> AppResult<impl IntoResponse> {
    let pid = roadmap::epic_project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(name) = &req.name {
        check_name(name, "epic")?;
    }
    if let Some(color) = &req.color {
        check_color(color)?;
    }
    // The effective dates after this patch, for the range check.
    let epic = roadmap::epic_update(
        &state.db,
        id,
        pid,
        req.name.as_deref().map(str::trim),
        req.color.as_deref(),
        req.start_date.is_some(),
        req.start_date.flatten(),
        req.end_date.is_some(),
        req.end_date.flatten(),
    )
    .await?;
    check_range(epic.start_date, epic.end_date)?;
    Ok(Json(epic))
}

async fn delete_epic(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = roadmap::epic_project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    roadmap::epic_delete(&state.db, id, pid).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── milestones ─────────────────────────────────────────────────────────────

async fn list_milestones(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(roadmap::milestones_list(&state.db, ctx.id).await?))
}

async fn create_milestone(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateMilestoneReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    check_name(&req.name, "milestone")?;
    let m = roadmap::milestone_create(&state.db, ctx.id, req.name.trim(), req.due_date).await?;
    Ok((StatusCode::CREATED, Json(m)))
}

async fn update_milestone(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateMilestoneReq>,
) -> AppResult<impl IntoResponse> {
    let pid = roadmap::milestone_project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(name) = &req.name {
        check_name(name, "milestone")?;
    }
    let m = roadmap::milestone_update(
        &state.db,
        id,
        pid,
        req.name.as_deref().map(str::trim),
        req.due_date,
    )
    .await?;
    Ok(Json(m))
}

async fn delete_milestone(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = roadmap::milestone_project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    roadmap::milestone_delete(&state.db, id, pid).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── task → epic ────────────────────────────────────────────────────────────

async fn assign_epic(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<AssignEpicReq>,
) -> AppResult<impl IntoResponse> {
    let (project_id, project_key) = resolve_project_from_task_key(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    // An epic can only group tasks from its own project.
    if let Some(epic_id) = req.epic_id {
        if roadmap::epic_project_of(&state.db, epic_id).await? != project_id {
            return Err(AppError::BadRequest(
                "epic belongs to a different project".into(),
            ));
        }
    }
    let task_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM tasks WHERE key = $1 AND project_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(&task_key)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    roadmap::assign_task_epic(&state.db, task_id, req.epic_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
