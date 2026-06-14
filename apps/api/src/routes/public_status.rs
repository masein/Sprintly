//! Public read-only status pages (F18).
//!
//!   GET    /public/status/:token        — UNAUTHENTICATED whitelisted summary
//!   GET    /projects/:key/public-status — lead: current on/off + URL
//!   POST   /projects/:key/public-status — lead: enable, returns token + URL
//!   DELETE /projects/:key/public-status — lead: disable (invalidates the URL)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx, public_status,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/status/:token", get(public_view))
        .route(
            "/projects/:key/public-status",
            get(status).post(enable).delete(disable),
        )
}

/// Unauthenticated: no `CurrentUser` extractor → open to the world.
async fn public_view(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<impl IntoResponse> {
    Ok(Json(public_status::load_by_token(&state.db, &token).await?))
}

#[derive(Debug, Serialize)]
struct AdminStatus {
    enabled: bool,
    token: Option<String>,
    url: Option<String>,
}

fn public_url(cfg: &crate::config::Config, token: &str) -> String {
    format!("{}/status/{token}", cfg.public_url.trim_end_matches('/'))
}

async fn require_lead(state: &AppState, key: &str, user: &CurrentUser) -> AppResult<uuid::Uuid> {
    let ctx = project_ctx::load_by_key(&state.db, key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(ctx.id)
}

async fn status(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let project_id = require_lead(&state, &key, &user).await?;
    let token = public_status::current_token(&state.db, project_id).await?;
    Ok(Json(AdminStatus {
        enabled: token.is_some(),
        url: token.as_deref().map(|t| public_url(&state.cfg, t)),
        token,
    }))
}

async fn enable(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let project_id = require_lead(&state, &key, &user).await?;
    let token = public_status::enable(&state.db, project_id).await?;
    Ok(Json(AdminStatus {
        enabled: true,
        url: Some(public_url(&state.cfg, &token)),
        token: Some(token),
    }))
}

async fn disable(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let project_id = require_lead(&state, &key, &user).await?;
    public_status::disable(&state.db, project_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
