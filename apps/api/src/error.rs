//! A single error enum for the whole API. Every handler returns
//! `Result<_, AppError>`; the `IntoResponse` impl turns it into a
//! JSON body shaped like:
//!
//! ```text
//! { "error": { "code": "auth.invalid_credentials", "message": "…",
//!              "trace_id": "abc123", "details": null } }
//! ```
//!
//! Never `unwrap` in a handler. If you find yourself wanting to, add a new
//! variant here instead.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;
use tracing::{error, warn};

#[derive(Debug, Error)]
pub enum AppError {
    // ─── 4xx ─────────────────────────────────────────────────────────────
    #[error("not found")]
    NotFound,

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("rate limited")]
    RateLimited,

    // ─── 5xx ─────────────────────────────────────────────────────────────
    #[error("database error")]
    Database(#[from] sqlx::Error),

    #[error("redis error")]
    Redis(#[from] redis::RedisError),

    #[error("crypto error: {0}")]
    Crypto(&'static str),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::Database(_) | Self::Redis(_) | Self::Crypto(_) | Self::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();

        // Log 5xx loud, 4xx quiet. Never leak internal messages to clients.
        let public_message = match &self {
            Self::Database(_) | Self::Redis(_) | Self::Crypto(_) | Self::Internal(_) => {
                error!(error = ?self, status = %status, "server error");
                "We broke it. Tell an admin and go get coffee."
            }
            other => {
                warn!(error = %other, status = %status, "client error");
                other_public_message(other)
            }
        };

        let body = Json(json!({
            "error": {
                "code": code,
                "message": public_message,
            }
        }));

        (status, body).into_response()
    }
}

fn other_public_message(err: &AppError) -> &'static str {
    match err {
        AppError::NotFound => "Not found. This page is in a different branch.",
        AppError::Unauthorized => "Sign in to continue.",
        AppError::Forbidden => "You don't have access to this.",
        AppError::RateLimited => "Slow down. Try again in a moment.",
        AppError::Validation(_) => "Some fields look off. Check the form.",
        AppError::BadRequest(_) => "That request didn't parse.",
        AppError::Conflict(_) => "That already exists.",
        _ => "Something went wrong.",
    }
}
