//! The `CurrentUser` extractor.
//!
//! Reads the access token from either the `Authorization: Bearer ...` header
//! or the `sprintly_access` cookie (in that order), validates it, confirms
//! the session is still live in the DB, and exposes the user to handlers.
//!
//! The session-liveness check costs one cheap SELECT per authenticated
//! request. It's the cost of being able to revoke a logged-out user
//! immediately without waiting for their JWT to expire. Acceptable.

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header, request::Parts, StatusCode},
};
use uuid::Uuid;

use crate::{
    config::Config,
    domain::{permissions::Role, sessions, tokens},
    infra::AppState,
    AppError,
};

/// The actor on the current request. Cheap to clone.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: Role,
}

impl CurrentUser {
    pub fn as_actor(&self) -> crate::domain::permissions::Actor {
        crate::domain::permissions::Actor {
            id: self.id,
            role: self.role,
        }
    }
}

#[async_trait::async_trait]
impl<S> FromRequestParts<S> for CurrentUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        let token = extract_access_token(parts).ok_or(AppError::Unauthorized)?;

        let claims = tokens::verify_access(&state.cfg.auth, &token)?;

        // Session liveness — revoked sessions stop working before JWT expiry.
        if !sessions::session_is_live(&state.db, claims.sid).await? {
            return Err(AppError::Unauthorized);
        }

        let role = Role::parse(&claims.role).ok_or(AppError::Unauthorized)?;
        Ok(Self {
            id: claims.sub,
            session_id: claims.sid,
            role,
        })
    }
}

/// Header takes precedence so APIs called from cURL / shell scripts work the
/// same way as the browser. Cookie is the browser path.
fn extract_access_token(parts: &Parts) -> Option<String> {
    // Authorization: Bearer xxx
    if let Some(h) = parts.headers.get(header::AUTHORIZATION) {
        if let Ok(s) = h.to_str() {
            if let Some(rest) = s.strip_prefix("Bearer ") {
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    // Cookie: sprintly_access=xxx
    if let Some(h) = parts.headers.get(header::COOKIE) {
        if let Ok(raw) = h.to_str() {
            for kv in raw.split(';') {
                let kv = kv.trim();
                if let Some(rest) = kv.strip_prefix("sprintly_access=") {
                    if !rest.is_empty() {
                        return Some(rest.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Helper used by handlers that just want to bail with a clean 401.
#[allow(dead_code)]
pub fn unauthorized() -> (StatusCode, &'static str) {
    (StatusCode::UNAUTHORIZED, "unauthorized")
}

// Make AppState referenceable for the extractor when the router state IS
// AppState (the common case).
impl FromRef<AppState> for AppState {
    fn from_ref(s: &AppState) -> Self {
        s.clone()
    }
}

// Pull Config out of state for convenience in handlers.
impl FromRef<AppState> for std::sync::Arc<Config> {
    fn from_ref(s: &AppState) -> Self {
        s.cfg.clone()
    }
}
