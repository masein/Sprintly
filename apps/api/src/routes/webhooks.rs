//! Per-project outbound webhooks (F2).
//!
//! A webhook has a `target_type`: `outbound` (generic JSON, HMAC-signed with a
//! stored secret) or `slack`/`discord` (a formatted message POSTed to the URL,
//! which is itself the credential — see ADR 0002). Delivery runs in the jobs
//! worker with retry/backoff; every attempt is recorded in `webhook_deliveries`.
//!
//!   GET    /projects/:key/webhooks
//!   POST   /projects/:key/webhooks
//!   PATCH  /webhooks/:id
//!   DELETE /webhooks/:id
//!   GET    /webhooks/:id/deliveries      — recent delivery attempts
//!   POST   /webhooks/:id/test            — enqueue a synthetic test delivery

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx,
        vault::{encrypt, ProjectKey},
        webhooks as webhook_domain,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

const TARGET_TYPES: [&str; 3] = ["outbound", "slack", "discord"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/webhooks", get(list).post(create))
        .route("/webhooks/:id", axum::routing::patch(edit).delete(remove))
        .route("/webhooks/:id/deliveries", get(deliveries))
        .route("/webhooks/:id/test", post(send_test))
}

#[derive(Debug, Serialize)]
pub struct WebhookRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub url: String,
    pub target_type: String,
    pub events: Vec<String>,
    pub active: bool,
    pub last_delivery_at: Option<DateTime<Utc>>,
    pub last_status: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DeliveryRow {
    pub id: Uuid,
    pub event: String,
    pub status_code: Option<i32>,
    pub ok: bool,
    pub error: Option<String>,
    pub attempt: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateWebhookReq {
    #[validate(url)]
    pub url: String,
    /// Required for `outbound`; ignored for chat targets.
    #[validate(length(min = 8, max = 200))]
    pub secret: Option<String>,
    pub events: Vec<String>,
    pub target_type: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditWebhookReq {
    #[validate(url)]
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
    pub target_type: Option<String>,
    #[validate(length(min = 8, max = 200))]
    pub secret: Option<String>,
}

fn check_target_type(t: &str) -> AppResult<()> {
    if TARGET_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "target_type must be one of: {}",
            TARGET_TYPES.join(", ")
        )))
    }
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT id              AS "id!: Uuid",
               project_id      AS "project_id!: Uuid",
               url             AS "url!: String",
               target_type     AS "target_type!: String",
               events          AS "events!: Vec<String>",
               active          AS "active!: bool",
               last_delivery_at,
               last_status,
               created_at      AS "created_at!: DateTime<Utc>"
        FROM   webhooks
        WHERE  project_id = $1 AND deleted_at IS NULL
        ORDER  BY created_at DESC
        "#,
        ctx.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<WebhookRow> = rows
        .into_iter()
        .map(|r| WebhookRow {
            id: r.id,
            project_id: r.project_id,
            url: r.url,
            target_type: r.target_type,
            events: r.events,
            active: r.active,
            last_delivery_at: r.last_delivery_at,
            last_status: r.last_status,
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateWebhookReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let target_type = req.target_type.as_deref().unwrap_or("outbound");
    check_target_type(target_type)?;
    // Only the generic target signs, so only it needs a secret.
    let secret = req.secret.as_deref().filter(|s| !s.is_empty());
    if target_type == "outbound" && secret.is_none() {
        return Err(AppError::BadRequest(
            "an outbound webhook needs a signing secret".into(),
        ));
    }

    let id = Uuid::now_v7();
    // Encrypt the signing secret at rest under the per-project vault key
    // (AAD = webhook id), so the worker can recover it to sign deliveries.
    let enc = match secret {
        Some(s) => {
            let key = ProjectKey::derive(&state.cfg.vault.master_key, ctx.id, 1);
            Some(encrypt(&key, s.as_bytes(), id.as_bytes())?)
        }
        None => None,
    };
    sqlx::query(
        r#"
        INSERT INTO webhooks
            (id, project_id, url, target_type, secret_ciphertext, secret_nonce, events, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(id)
    .bind(ctx.id)
    .bind(&req.url)
    .bind(target_type)
    .bind(enc.as_ref().map(|(c, _)| c.as_slice()))
    .bind(enc.as_ref().map(|(_, n)| n.as_slice()))
    .bind(&req.events)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    let dto = fetch(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn edit(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<EditWebhookReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if let Some(t) = req.target_type.as_deref() {
        check_target_type(t)?;
    }
    let pid: Uuid =
        sqlx::query_scalar("SELECT project_id FROM webhooks WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    // Re-encrypt the secret only if a new one was supplied.
    let new_secret: Option<(Vec<u8>, Vec<u8>)> =
        match req.secret.as_deref().filter(|s| !s.is_empty()) {
            Some(s) => {
                let key = ProjectKey::derive(&state.cfg.vault.master_key, pid, 1);
                let (ct, nonce) = encrypt(&key, s.as_bytes(), id.as_bytes())?;
                Some((ct, nonce.to_vec()))
            }
            None => None,
        };
    sqlx::query(
        r#"
        UPDATE webhooks SET
            url         = COALESCE($2, url),
            events      = COALESCE($3, events),
            active      = COALESCE($4, active),
            target_type = COALESCE($5, target_type),
            secret_ciphertext = COALESCE($6, secret_ciphertext),
            secret_nonce      = COALESCE($7, secret_nonce)
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(req.url.as_deref())
    .bind(req.events.as_deref())
    .bind(req.active)
    .bind(req.target_type.as_deref())
    .bind(new_secret.as_ref().map(|(c, _)| c.as_slice()))
    .bind(new_secret.as_ref().map(|(_, n)| n.as_slice()))
    .execute(&state.db)
    .await?;
    let dto = fetch(&state.db, id).await?;
    Ok(Json(dto))
}

async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = webhook_project(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE webhooks SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn deliveries(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = webhook_project(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows: Vec<DeliveryRow> = sqlx::query_as(
        r#"SELECT id, event, status_code, ok, error, attempt, created_at
           FROM webhook_deliveries WHERE webhook_id = $1
           ORDER BY created_at DESC LIMIT 50"#,
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "items": rows })))
}

async fn send_test(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = webhook_project(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    webhook_domain::enqueue_test(&state.db, id).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Project a (live) webhook belongs to, or 404.
async fn webhook_project(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT project_id FROM webhooks WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn fetch(db: &PgPool, id: Uuid) -> AppResult<WebhookRow> {
    let r = sqlx::query!(
        r#"
        SELECT id              AS "id!: Uuid",
               project_id      AS "project_id!: Uuid",
               url             AS "url!: String",
               target_type     AS "target_type!: String",
               events          AS "events!: Vec<String>",
               active          AS "active!: bool",
               last_delivery_at,
               last_status,
               created_at      AS "created_at!: DateTime<Utc>"
        FROM   webhooks WHERE id = $1
        "#,
        id
    )
    .fetch_one(db)
    .await?;
    Ok(WebhookRow {
        id: r.id,
        project_id: r.project_id,
        url: r.url,
        target_type: r.target_type,
        events: r.events,
        active: r.active,
        last_delivery_at: r.last_delivery_at,
        last_status: r.last_status,
        created_at: r.created_at,
    })
}
