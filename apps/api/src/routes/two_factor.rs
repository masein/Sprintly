//! Two-factor auth management (F11), all scoped to the current user.
//!
//!   GET  /me/2fa            — status { enabled, has_secret, required }
//!   POST /me/2fa/enroll     — start: mint a secret, return otpauth URI + base32
//!   POST /me/2fa/activate   — confirm a code → enable + return recovery codes
//!   POST /me/2fa/disable    — turn off (requires a current code or recovery)
//!
//! The login step-up itself (`POST /auth/2fa`) lives in `routes::auth` next to
//! session issuance. Wrong-code attempts here and there share the H1 rate
//! limiter, keyed per user.

use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    domain::{totp, two_factor},
    infra::AppState,
    middleware::{rate_limit, CurrentUser},
    AppError, AppResult,
};

const ISSUER: &str = "Sprintly";
const RECOVERY_CODE_COUNT: usize = 10;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/2fa", get(status))
        .route("/me/2fa/enroll", post(enroll))
        .route("/me/2fa/activate", post(activate))
        .route("/me/2fa/disable", post(disable))
}

#[derive(Debug, Serialize)]
struct StatusDto {
    enabled: bool,
    has_secret: bool,
    /// The org has 2FA marked as required (advisory nudge in the UI).
    required: bool,
}

async fn status(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    let s = two_factor::status(&state.db, user.id).await?;
    Ok(Json(StatusDto {
        enabled: s.enabled,
        has_secret: s.has_secret,
        required: state.cfg.require_2fa,
    }))
}

#[derive(Debug, Serialize)]
struct EnrollDto {
    /// base32 secret for manual entry into an authenticator app.
    secret: String,
    /// `otpauth://` URI to render as a QR code.
    otpauth_uri: String,
}

async fn enroll(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    // Re-enrolling while active would silently invalidate the working app and
    // recovery codes — make the user disable first.
    if two_factor::status(&state.db, user.id).await?.enabled {
        return Err(AppError::Conflict(
            "two-factor is already on — disable it first to re-enrol".into(),
        ));
    }

    let secret = totp::generate_secret();
    two_factor::enroll_pending(&state.db, user.id, &secret).await?;

    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(EnrollDto {
        secret: totp::base32_encode(&secret),
        otpauth_uri: totp::provisioning_uri(&secret, ISSUER, &email),
    }))
}

#[derive(Debug, Deserialize)]
struct CodeReq {
    code: String,
}

#[derive(Debug, Serialize)]
struct ActivateDto {
    /// Single-use recovery codes — shown exactly once.
    recovery_codes: Vec<String>,
}

async fn activate(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CodeReq>,
) -> AppResult<impl IntoResponse> {
    rate_limit::hit(
        &state,
        &format!("sprintly:rl:2fa:{}", user.id),
        rate_limit::twofa_per_min(),
        60,
    )
    .await?;

    let secret = two_factor::pending_secret(&state.db, user.id)
        .await?
        .ok_or_else(|| AppError::BadRequest("start enrolment first".into()))?;

    let now = chrono::Utc::now().timestamp() as u64;
    if !totp::verify(&secret, &req.code, now, 1) {
        return Err(AppError::Unauthorized);
    }

    // Mint recovery codes, store only their hashes.
    let codes = totp::generate_recovery_codes(RECOVERY_CODE_COUNT);
    let hashes: Vec<String> = codes.iter().map(|c| totp::hash_recovery_code(c)).collect();
    if !two_factor::activate(&state.db, user.id, &hashes).await? {
        // Nothing pending to activate (already on, or race) — surface honestly.
        return Err(AppError::Conflict("two-factor is already on".into()));
    }

    Ok(Json(ActivateDto {
        recovery_codes: codes,
    }))
}

async fn disable(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CodeReq>,
) -> AppResult<impl IntoResponse> {
    rate_limit::hit(
        &state,
        &format!("sprintly:rl:2fa:{}", user.id),
        rate_limit::twofa_per_min(),
        60,
    )
    .await?;

    let secret = two_factor::secret_if_enabled(&state.db, user.id)
        .await?
        .ok_or_else(|| AppError::BadRequest("two-factor isn't on".into()))?;

    // Accept either a live TOTP code or a recovery code to turn it off.
    let now = chrono::Utc::now().timestamp() as u64;
    let ok = totp::verify(&secret, &req.code, now, 1)
        || two_factor::consume_recovery_code(&state.db, user.id, &req.code).await?;
    if !ok {
        return Err(AppError::Unauthorized);
    }

    two_factor::disable(&state.db, user.id).await?;
    Ok(Json(json!({ "disabled": true })))
}
