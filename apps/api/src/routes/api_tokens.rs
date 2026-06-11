//! Personal API tokens.
//!
//!   GET    /me/tokens       — list (names + metadata, never secrets)
//!   POST   /me/tokens       — { name, scopes?, expires_at? } → token + secret (once)
//!   DELETE /me/tokens/:id   — revoke, effective immediately

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::{domain::api_tokens, infra::AppState, middleware::CurrentUser, AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/tokens", get(list).post(create))
        .route("/me/tokens/:id", axum::routing::delete(revoke))
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    name: String,
    /// Defaults to read-only; pass ["read","write"] (or just ["write"]) for
    /// write access.
    scopes: Option<Vec<String>>,
    expires_at: Option<DateTime<Utc>>,
}

async fn list(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    Ok(Json(api_tokens::list(&state.db, user.id).await?))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateReq>,
) -> AppResult<impl IntoResponse> {
    let name = req.name.trim();
    if name.is_empty() || name.len() > 60 {
        return Err(AppError::BadRequest("token name must be 1–60 chars".into()));
    }
    let scopes = req.scopes.unwrap_or_else(|| vec!["read".to_string()]);
    if !api_tokens::valid_scopes(&scopes) {
        return Err(AppError::BadRequest(format!(
            "scopes must be a non-empty subset of: {}",
            api_tokens::SCOPES.join(", ")
        )));
    }
    if req.expires_at.is_some_and(|e| e <= Utc::now()) {
        return Err(AppError::BadRequest(
            "expires_at is in the past — that token would be born dead".into(),
        ));
    }
    let (token, secret) =
        api_tokens::create(&state.db, user.id, name, &scopes, req.expires_at).await?;
    // The secret appears in exactly one response, ever. Copy it now.
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "token": token, "secret": secret })),
    ))
}

async fn revoke(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    api_tokens::revoke(&state.db, user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
