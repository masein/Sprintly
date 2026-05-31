//! Webhooks — scaffolding only for v1.
//!
//! Storage + CRUD work. Outbound delivery is **not wired**. Rows can be
//! created, edited, listed, deleted; the API doesn't send anything. The
//! admin UI tags this surface with a "Coming soon" badge.
//!
//! When delivery lands later, this module gets a `dispatch_event(event)`
//! helper that fans out to active rows matching the `events` array.
//!
//!   GET    /projects/:key/webhooks
//!   POST   /projects/:key/webhooks
//!   PATCH  /webhooks/:id
//!   DELETE /webhooks/:id

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/:key/webhooks",
            get(list).post(create),
        )
        .route(
            "/webhooks/:id",
            axum::routing::patch(edit).delete(remove),
        )
}

#[derive(Debug, Serialize)]
pub struct WebhookRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub url: String,
    pub events: Vec<String>,
    pub active: bool,
    pub last_delivery_at: Option<DateTime<Utc>>,
    pub last_status: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateWebhookReq {
    #[validate(url)]
    pub url: String,
    #[validate(length(min = 8, max = 200))]
    pub secret: String,
    pub events: Vec<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditWebhookReq {
    #[validate(url)]
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
    #[validate(length(min = 8, max = 200))]
    pub secret: Option<String>,
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
    let id = Uuid::now_v7();
    let mut h = Sha256::new();
    h.update(req.secret.as_bytes());
    let hash: [u8; 32] = h.finalize().into();
    sqlx::query(
        r#"
        INSERT INTO webhooks (id, project_id, url, secret_hash, events, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(ctx.id)
    .bind(&req.url)
    .bind(hash.as_slice())
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
    let pid: Uuid = sqlx::query_scalar(
        "SELECT project_id FROM webhooks WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let new_hash = req.secret.as_ref().map(|s| {
        let mut h = Sha256::new();
        h.update(s.as_bytes());
        let arr: [u8; 32] = h.finalize().into();
        arr.to_vec()
    });
    sqlx::query(
        r#"
        UPDATE webhooks SET
            url    = COALESCE($2, url),
            events = COALESCE($3, events),
            active = COALESCE($4, active),
            secret_hash = COALESCE($5, secret_hash)
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(req.url.as_deref())
    .bind(req.events.as_deref())
    .bind(req.active)
    .bind(new_hash.as_deref())
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
    let pid: Uuid = sqlx::query_scalar(
        "SELECT project_id FROM webhooks WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
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

async fn fetch(db: &PgPool, id: Uuid) -> AppResult<WebhookRow> {
    let r = sqlx::query!(
        r#"
        SELECT id              AS "id!: Uuid",
               project_id      AS "project_id!: Uuid",
               url             AS "url!: String",
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
        events: r.events,
        active: r.active,
        last_delivery_at: r.last_delivery_at,
        last_status: r.last_status,
        created_at: r.created_at,
    })
}
