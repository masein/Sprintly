//! Admin-only endpoints. M1 ships invite-token management; suspend/delete
//! user and audit-log views land in M10.
//!
//!   POST /admin/invites              — generate a one-shot invite link.
//!                                       The plaintext token is returned ONCE
//!                                       in this response. We only store the
//!                                       hash. If you lose it, revoke and
//!                                       re-issue.
//!   GET  /admin/invites              — list outstanding invites (no plaintext).
//!   POST /admin/invites/:id/revoke   — burn an unused invite.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::permissions::{can, Action, Resource},
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/invites", post(create_invite).get(list_invites))
        .route("/admin/invites/:id/revoke", post(revoke_invite))
}

#[derive(Debug, Deserialize)]
pub struct CreateInviteReq {
    pub email_hint: Option<String>,
    /// Defaults to "member" if omitted. Must be one of admin/member/viewer.
    pub suggested_role: Option<String>,
    /// Time-to-live in hours. Defaults to 168 (one week). Min 1, max 720 (30d).
    pub ttl_hours: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreateInviteResp {
    pub id: Uuid,
    /// Surfaced ONCE — never returned again. Show it as a copy-paste link.
    pub token: String,
    pub url: String,
    pub email_hint: Option<String>,
    pub suggested_role: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct InviteRow {
    pub id: Uuid,
    pub email_hint: Option<String>,
    pub suggested_role: String,
    pub invited_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub consumed_by: Option<Uuid>,
}

async fn create_invite(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateInviteReq>,
) -> AppResult<impl IntoResponse> {
    if !can(&user.as_actor(), Action::InviteUser, Resource::Admin) {
        return Err(AppError::Forbidden);
    }

    let role = match req.suggested_role.as_deref() {
        None | Some("member") => "member",
        Some("admin") => "admin",
        Some("viewer") => "viewer",
        Some(_) => {
            return Err(AppError::BadRequest(
                "suggested_role must be admin/member/viewer".into(),
            ))
        }
    };

    let ttl_hours = req.ttl_hours.unwrap_or(168).clamp(1, 720);
    let expires_at = Utc::now() + Duration::hours(ttl_hours);

    // Mint a fresh 32-byte token. Store sha256; return plaintext once.
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    let token_hash: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(raw);
        h.finalize().into()
    };

    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO invite_tokens (id, token_hash, email_hint, suggested_role, invited_by, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(token_hash.as_slice())
    .bind(req.email_hint.as_deref())
    .bind(role)
    .bind(user.id)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    // Build the canonical signup URL using the configured public base.
    let url = format!(
        "{base}/register?invite={token}",
        base = state.cfg.public_url.trim_end_matches('/'),
    );

    Ok((
        StatusCode::CREATED,
        Json(CreateInviteResp {
            id,
            token,
            url,
            email_hint: req.email_hint,
            suggested_role: role.to_string(),
            expires_at,
        }),
    ))
}

async fn list_invites(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    if !can(&user.as_actor(), Action::InviteUser, Resource::Admin) {
        return Err(AppError::Forbidden);
    }

    let rows = sqlx::query!(
        r#"
        SELECT id              AS "id!: Uuid",
               email_hint,
               suggested_role  AS "suggested_role!: String",
               invited_by,
               created_at      AS "created_at!: DateTime<Utc>",
               expires_at      AS "expires_at!: DateTime<Utc>",
               consumed_at,
               consumed_by
        FROM   invite_tokens
        ORDER  BY created_at DESC
        LIMIT  200
        "#
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<InviteRow> = rows
        .into_iter()
        .map(|r| InviteRow {
            id: r.id,
            email_hint: r.email_hint,
            suggested_role: r.suggested_role,
            invited_by: r.invited_by,
            created_at: r.created_at,
            expires_at: r.expires_at,
            consumed_at: r.consumed_at,
            consumed_by: r.consumed_by,
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items })))
}

async fn revoke_invite(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    if !can(&user.as_actor(), Action::InviteUser, Resource::Admin) {
        return Err(AppError::Forbidden);
    }

    // "Revoke" = mark expired-in-the-past so /auth/register can never accept
    // it. We don't delete the row — the audit trail matters.
    let result = sqlx::query(
        r#"
        UPDATE invite_tokens
           SET expires_at = LEAST(expires_at, now())
         WHERE id = $1 AND consumed_at IS NULL
        "#,
    )
    .bind(id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
