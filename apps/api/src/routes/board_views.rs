//! Saved board views (F8).
//!
//!   GET    /projects/:key/board-views     — views the caller can see (own + shared)
//!   POST   /projects/:key/board-views     — save a view (any project member)
//!   PATCH  /board-views/:id               — owner only
//!   DELETE /board-views/:id               — owner only

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domain::{
        board_views,
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/board-views", get(list).post(create))
        .route(
            "/board-views/:id",
            axum::routing::patch(update).delete(remove),
        )
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    name: String,
    #[serde(default)]
    filter: Value,
    group_by: Option<String>,
    shared: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    name: Option<String>,
    filter: Option<Value>,
    group_by: Option<String>,
    shared: Option<bool>,
}

fn check_name(name: &str) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > 80 {
        return Err(AppError::BadRequest("view name must be 1–80 chars".into()));
    }
    Ok(())
}

fn check_group_by(g: &str) -> AppResult<()> {
    if board_views::valid_group_by(g) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "group_by must be one of: {}",
            board_views::GROUP_BYS.join(", ")
        )))
    }
}

/// The opaque filter blob is the client's, but cap its size so a view row
/// can't be used to stash arbitrary data.
fn check_filter(filter: &Value) -> AppResult<()> {
    if filter.is_null() {
        return Ok(());
    }
    if !filter.is_array() {
        return Err(AppError::BadRequest("filter must be a JSON array".into()));
    }
    if filter.to_string().len() > 4096 {
        return Err(AppError::BadRequest("filter is too large".into()));
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
    Ok(Json(board_views::list(&state.db, ctx.id, user.id).await?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    // Anyone who can view the board can save a view (it's theirs).
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    check_name(&req.name)?;
    let group_by = req.group_by.as_deref().unwrap_or("none");
    check_group_by(group_by)?;
    let filter = if req.filter.is_null() {
        Value::Array(vec![])
    } else {
        req.filter
    };
    check_filter(&filter)?;
    let view = board_views::create(
        &state.db,
        ctx.id,
        user.id,
        req.name.trim(),
        &filter,
        group_by,
        req.shared.unwrap_or(false),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateReq>,
) -> AppResult<impl IntoResponse> {
    if let Some(name) = &req.name {
        check_name(name)?;
    }
    if let Some(g) = &req.group_by {
        check_group_by(g)?;
    }
    if let Some(f) = &req.filter {
        check_filter(f)?;
    }
    let view = board_views::update(
        &state.db,
        id,
        user.id,
        req.name.as_deref().map(str::trim),
        req.filter.as_ref(),
        req.group_by.as_deref(),
        req.shared,
    )
    .await?;
    Ok(Json(view))
}

async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    board_views::delete(&state.db, id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
