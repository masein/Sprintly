//! Per-project label registry.
//!
//!   GET    /projects/:key/labels
//!   POST   /projects/:key/labels      { name, color? }
//!   PATCH  /labels/:id                { name?, color? }
//!   DELETE /labels/:id

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        labels,
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/labels", get(list).post(create))
        .route("/labels/:id", axum::routing::patch(update).delete(remove))
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    name: String,
    color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    name: Option<String>,
    color: Option<String>,
}

fn check_name(name: &str) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > 40 {
        return Err(AppError::BadRequest("label name must be 1–40 chars".into()));
    }
    Ok(())
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(labels::list(&state.db, ctx.id).await?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    check_name(&req.name)?;
    let color = req.color.as_deref().unwrap_or("#7c5cff");
    if !labels::valid_color(color) {
        return Err(AppError::BadRequest("color must be #rgb or #rrggbb".into()));
    }
    let label = labels::create(&state.db, ctx.id, req.name.trim(), color).await?;
    Ok((StatusCode::CREATED, Json(label)))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateReq>,
) -> AppResult<impl IntoResponse> {
    let pid = labels::project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(name) = &req.name {
        check_name(name)?;
    }
    if let Some(color) = &req.color {
        if !labels::valid_color(color) {
            return Err(AppError::BadRequest("color must be #rgb or #rrggbb".into()));
        }
    }
    let label = labels::update(
        &state.db,
        id,
        pid,
        req.name.as_deref().map(str::trim),
        req.color.as_deref(),
    )
    .await?;
    Ok(Json(label))
}

async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = labels::project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    labels::delete(&state.db, id, pid).await?;
    Ok(StatusCode::NO_CONTENT)
}
