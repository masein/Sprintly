//! Vault endpoints.
//!
//!   POST   /projects/:key/vault             — create item (accepts plaintext, encrypts)
//!   GET    /projects/:key/vault             — list items (no ciphertext, no plaintext)
//!   GET    /vault/:id                       — metadata (no ciphertext)
//!   PATCH  /vault/:id                       — edit; if `value` set, re-encrypt + rotate row
//!   DELETE /vault/:id                       — soft delete
//!   POST   /vault/:id/reveal                — decrypt + return plaintext ONCE. Rate-limited.
//!   POST   /vault/:id/copied                — client signals "I copied this". Audited.
//!   GET    /vault/:id/access                — list grants
//!   POST   /vault/:id/access                — grant view/edit to a user
//!   DELETE /vault/:id/access/:user_id       — revoke
//!   GET    /vault/:id/audit                 — audit log (lead/admin/item editor)
//!
//! Access model (layered on top of the project role):
//!   * Global admin: can do anything to any vault item.
//!   * Project lead: implicitly can view + edit + grant on every item in
//!     the project. Bypasses `vault_access`.
//!   * Project contributor: needs an explicit `vault_access` row.
//!   * Project watcher: never.
//!   * Viewer (global role) cannot reveal anything.
//!
//! Reveal rate limit: a Redis token bucket per (user_id) with a 10/hour cap.
//! Exceeded → 429. We still write a 'revealed' audit row only on success.

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action, ProjectRole, Role as GlobalRole},
        projects as project_ctx,
        vault as vault_crypto,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

const REVEAL_LIMIT_PER_HOUR: u32 = 10;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/:key/vault",
            post(create_item).get(list_items),
        )
        .route(
            "/vault/:id",
            get(get_item).patch(edit_item).delete(delete_item),
        )
        .route("/vault/:id/reveal", post(reveal_item))
        .route("/vault/:id/copied", post(mark_copied))
        .route("/vault/:id/access", get(list_access).post(grant_access))
        .route("/vault/:id/access/:user_id", delete(revoke_access))
        .route("/vault/:id/audit", get(list_audit))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct VaultItemDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_key: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub key_version: i32,
    pub created_by: Option<Uuid>,
    pub last_rotated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateItemReq {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    pub kind: String,
    #[validate(length(max = 4000))]
    pub description: Option<String>,
    /// The plaintext secret. Discarded server-side after encryption.
    #[validate(length(min = 1, max = 64 * 1024))]
    pub value: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditItemReq {
    #[validate(length(min = 1, max = 120))]
    pub name: Option<String>,
    #[validate(length(max = 4000))]
    pub description: Option<String>,
    /// If provided, the row is re-encrypted under a fresh nonce. Key version
    /// follows the current `SPRINTLY_VAULT_KEY_VERSION` so writes naturally
    /// migrate to the latest key.
    #[validate(length(min = 1, max = 64 * 1024))]
    pub value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RevealResp {
    pub id: Uuid,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct AccessRow {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub can_view: bool,
    pub can_edit: bool,
    pub granted_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct GrantAccessReq {
    pub user_id: Uuid,
    pub can_view: Option<bool>,
    pub can_edit: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct AuditRow {
    pub id: Uuid,
    pub user_handle: Option<String>,
    pub action: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn create_item(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(project_key): Path<String>,
    Json(req): Json<CreateItemReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if !valid_kind(&req.kind) {
        return Err(AppError::BadRequest("kind invalid".into()));
    }
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can_manage_vault(&user, &ctx) {
        return Err(AppError::Forbidden);
    }

    let item_id = Uuid::now_v7();
    let key_version = state.cfg.vault.key_version;
    let pkey = vault_crypto::ProjectKey::derive(&state.cfg.vault.master_key, ctx.id, key_version);
    let aad = item_id.as_bytes(); // bind ciphertext to the row's identity
    let (ciphertext, nonce) =
        vault_crypto::encrypt(&pkey, req.value.as_bytes(), aad)?;

    let mut tx = state.db.begin().await?;
    let insert = sqlx::query(
        r#"
        INSERT INTO vault_items
            (id, project_id, name, kind, description, encrypted_payload, nonce, key_version, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(item_id)
    .bind(ctx.id)
    .bind(&req.name)
    .bind(&req.kind)
    .bind(req.description.as_deref().unwrap_or(""))
    .bind(&ciphertext)
    .bind(nonce.as_slice())
    .bind(key_version)
    .bind(user.id)
    .execute(&mut *tx)
    .await;

    if let Err(sqlx::Error::Database(e)) = &insert {
        if e.is_unique_violation() {
            return Err(AppError::Conflict(
                "vault item with that name already exists in this project".into(),
            ));
        }
    }
    insert?;

    write_audit(&mut tx, item_id, Some(user.id), "created", &headers, addr).await?;
    tx.commit().await?;

    let dto = fetch_item(&state.db, item_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn list_items(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let user_is_lead_or_admin = user.role == GlobalRole::Admin
        || ctx.actor_role == Some(ProjectRole::Lead);

    let rows = sqlx::query!(
        r#"
        SELECT v.id              AS "id!: Uuid",
               v.project_id      AS "project_id!: Uuid",
               p.key             AS "project_key!: String",
               v.name            AS "name!: String",
               v.kind            AS "kind!: String",
               v.description     AS "description!: String",
               v.key_version     AS "key_version!: i32",
               v.created_by,
               v.last_rotated_at AS "last_rotated_at!: DateTime<Utc>",
               v.created_at      AS "created_at!: DateTime<Utc>",
               v.updated_at      AS "updated_at!: DateTime<Utc>",
               EXISTS(
                   SELECT 1 FROM vault_access a
                    WHERE a.vault_item_id = v.id AND a.user_id = $2
                      AND a.can_view = true
               ) AS "explicit_view!: bool"
        FROM   vault_items v
        JOIN   projects p ON p.id = v.project_id
        WHERE  v.project_id = $1 AND v.deleted_at IS NULL
        ORDER  BY v.kind, v.name
        "#,
        ctx.id,
        user.id,
    )
    .fetch_all(&state.db)
    .await?;

    // Contributors only see items they were explicitly granted.
    let items: Vec<VaultItemDto> = rows
        .into_iter()
        .filter(|r| user_is_lead_or_admin || r.explicit_view)
        .map(|r| VaultItemDto {
            id: r.id,
            project_id: r.project_id,
            project_key: r.project_key,
            name: r.name,
            kind: r.kind,
            description: r.description,
            key_version: r.key_version,
            created_by: r.created_by,
            last_rotated_at: r.last_rotated_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items })))
}

async fn get_item(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let (ctx, _can_edit) = require_access(&state.db, &user, id, AccessNeed::View).await?;
    let _ = ctx; // unused
    let dto = fetch_item(&state.db, id).await?;
    Ok(Json(dto))
}

async fn edit_item(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    Json(req): Json<EditItemReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let (ctx, _) = require_access(&state.db, &user, id, AccessNeed::Edit).await?;

    let mut tx = state.db.begin().await?;
    let mut new_ct: Option<Vec<u8>> = None;
    let mut new_nonce: Option<[u8; 24]> = None;
    let mut new_version: Option<i32> = None;
    if let Some(value) = req.value.as_deref() {
        let key_version = state.cfg.vault.key_version;
        let pkey = vault_crypto::ProjectKey::derive(
            &state.cfg.vault.master_key,
            ctx.id,
            key_version,
        );
        let (ct, nonce) = vault_crypto::encrypt(&pkey, value.as_bytes(), id.as_bytes())?;
        new_ct = Some(ct);
        new_nonce = Some(nonce);
        new_version = Some(key_version);
    }

    sqlx::query(
        r#"
        UPDATE vault_items SET
            name              = COALESCE($2, name),
            description       = COALESCE($3, description),
            encrypted_payload = COALESCE($4, encrypted_payload),
            nonce             = COALESCE($5, nonce),
            key_version       = COALESCE($6, key_version),
            last_rotated_at   = CASE WHEN $4 IS NOT NULL THEN now() ELSE last_rotated_at END
         WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(req.name.as_deref())
    .bind(req.description.as_deref())
    .bind(new_ct.as_deref())
    .bind(new_nonce.as_ref().map(|n| n.as_slice()))
    .bind(new_version)
    .execute(&mut *tx)
    .await?;

    let action = if new_ct.is_some() { "rotated" } else { "edited" };
    write_audit(&mut tx, id, Some(user.id), action, &headers, addr).await?;
    tx.commit().await?;

    let dto = fetch_item(&state.db, id).await?;
    Ok(Json(dto))
}

async fn delete_item(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let (_, _) = require_access(&state.db, &user, id, AccessNeed::Edit).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE vault_items SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    write_audit(&mut tx, id, Some(user.id), "deleted", &headers, addr).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reveal_item(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    if user.role == GlobalRole::Viewer {
        return Err(AppError::Forbidden);
    }

    // Rate limit BEFORE any DB / crypto work.
    if !rate_limit_ok(&state, user.id).await? {
        return Err(AppError::RateLimited);
    }

    let (ctx, _) = require_access(&state.db, &user, id, AccessNeed::View).await?;

    let row = sqlx::query!(
        r#"
        SELECT encrypted_payload AS "encrypted_payload!: Vec<u8>",
               nonce             AS "nonce!: Vec<u8>",
               key_version       AS "key_version!: i32"
        FROM   vault_items
        WHERE  id = $1 AND deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let pkey = vault_crypto::ProjectKey::derive(
        &state.cfg.vault.master_key,
        ctx.id,
        row.key_version,
    );
    let plaintext_bytes =
        vault_crypto::decrypt(&pkey, &row.encrypted_payload, &row.nonce, id.as_bytes())?;
    let plaintext = String::from_utf8(plaintext_bytes)
        .map_err(|_| AppError::Crypto("vault payload not valid UTF-8"))?;

    // Audit AFTER successful decrypt — failed decrypts are interesting and
    // get their own logging path (server logs), not the audit table.
    let mut tx = state.db.begin().await?;
    write_audit(&mut tx, id, Some(user.id), "revealed", &headers, addr).await?;
    tx.commit().await?;

    Ok(Json(RevealResp { id, value: plaintext }))
}

async fn mark_copied(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_access(&state.db, &user, id, AccessNeed::View).await?;
    let mut tx = state.db.begin().await?;
    write_audit(&mut tx, id, Some(user.id), "copied", &headers, addr).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_access(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_access(&state.db, &user, id, AccessNeed::Edit).await?;
    let rows = sqlx::query!(
        r#"
        SELECT a.user_id      AS "user_id!: Uuid",
               u.handle       AS "handle!: String",
               u.display_name AS "display_name!: String",
               a.can_view     AS "can_view!: bool",
               a.can_edit     AS "can_edit!: bool",
               a.granted_at   AS "granted_at!: DateTime<Utc>"
        FROM   vault_access a
        JOIN   users u ON u.id = a.user_id
        WHERE  a.vault_item_id = $1 AND u.deleted_at IS NULL
        ORDER  BY u.handle
        "#,
        id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<AccessRow> = rows
        .into_iter()
        .map(|r| AccessRow {
            user_id: r.user_id,
            handle: r.handle,
            display_name: r.display_name,
            can_view: r.can_view,
            can_edit: r.can_edit,
            granted_at: r.granted_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn grant_access(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    Json(req): Json<GrantAccessReq>,
) -> AppResult<impl IntoResponse> {
    require_access(&state.db, &user, id, AccessNeed::Edit).await?;
    let can_view = req.can_view.unwrap_or(true);
    let can_edit = req.can_edit.unwrap_or(false);
    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO vault_access (vault_item_id, user_id, can_view, can_edit, granted_by)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (vault_item_id, user_id) DO UPDATE SET
            can_view = EXCLUDED.can_view,
            can_edit = EXCLUDED.can_edit,
            granted_by = EXCLUDED.granted_by,
            granted_at = now()
        "#,
    )
    .bind(id)
    .bind(req.user_id)
    .bind(can_view)
    .bind(can_edit)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    write_audit(&mut tx, id, Some(user.id), "access_granted", &headers, addr).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_access(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path((id, target)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    require_access(&state.db, &user, id, AccessNeed::Edit).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM vault_access WHERE vault_item_id = $1 AND user_id = $2")
        .bind(id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    write_audit(&mut tx, id, Some(user.id), "access_revoked", &headers, addr).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_audit(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_access(&state.db, &user, id, AccessNeed::Edit).await?;
    let rows = sqlx::query!(
        r#"
        SELECT a.id              AS "id!: Uuid",
               u.handle          AS "user_handle?: String",
               a.action          AS "action!: String",
               a.ip,
               a.user_agent,
               a.occurred_at     AS "occurred_at!: DateTime<Utc>"
        FROM   vault_audit_log a
        LEFT JOIN users u ON u.id = a.user_id
        WHERE  a.vault_item_id = $1
        ORDER  BY a.occurred_at DESC
        LIMIT  200
        "#,
        id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<AuditRow> = rows
        .into_iter()
        .map(|r| AuditRow {
            id: r.id,
            user_handle: r.user_handle,
            action: r.action,
            ip: r.ip.map(|i| i.to_string()),
            user_agent: r.user_agent,
            occurred_at: r.occurred_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

// ─── helpers ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum AccessNeed {
    View,
    Edit,
}

/// Resolve the project context for a vault item and gate on (view|edit).
/// Returns `(project_ctx, can_edit_flag)` for convenience.
async fn require_access(
    db: &PgPool,
    user: &CurrentUser,
    item_id: Uuid,
    need: AccessNeed,
) -> AppResult<(project_ctx::ProjectContext, bool)> {
    let row = sqlx::query!(
        r#"
        SELECT v.project_id   AS "project_id!: Uuid",
               a.can_view,
               a.can_edit
        FROM   vault_items v
        LEFT JOIN vault_access a
               ON a.vault_item_id = v.id AND a.user_id = $2
        WHERE  v.id = $1 AND v.deleted_at IS NULL
        "#,
        item_id,
        user.id
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    let ctx = project_ctx::load_by_id(db, row.project_id, user.id).await?;
    let is_admin = user.role == GlobalRole::Admin;
    let is_lead = ctx.actor_role == Some(ProjectRole::Lead);
    let explicit_view = row.can_view.unwrap_or(false);
    let explicit_edit = row.can_edit.unwrap_or(false);

    let allowed = match need {
        AccessNeed::View => is_admin || is_lead || explicit_view,
        AccessNeed::Edit => is_admin || is_lead || explicit_edit,
    };
    if !allowed {
        return Err(AppError::Forbidden);
    }
    Ok((ctx, is_admin || is_lead || explicit_edit))
}

fn can_manage_vault(user: &CurrentUser, ctx: &project_ctx::ProjectContext) -> bool {
    user.role == GlobalRole::Admin || ctx.actor_role == Some(ProjectRole::Lead)
}

fn valid_kind(s: &str) -> bool {
    matches!(s, "password" | "api_key" | "ssh_key" | "note" | "env_file")
}

async fn fetch_item(db: &PgPool, id: Uuid) -> AppResult<VaultItemDto> {
    let r = sqlx::query!(
        r#"
        SELECT v.id              AS "id!: Uuid",
               v.project_id      AS "project_id!: Uuid",
               p.key             AS "project_key!: String",
               v.name            AS "name!: String",
               v.kind            AS "kind!: String",
               v.description     AS "description!: String",
               v.key_version     AS "key_version!: i32",
               v.created_by,
               v.last_rotated_at AS "last_rotated_at!: DateTime<Utc>",
               v.created_at      AS "created_at!: DateTime<Utc>",
               v.updated_at      AS "updated_at!: DateTime<Utc>"
        FROM   vault_items v
        JOIN   projects p ON p.id = v.project_id
        WHERE  v.id = $1 AND v.deleted_at IS NULL
        "#,
        id
    )
    .fetch_one(db)
    .await?;
    Ok(VaultItemDto {
        id: r.id,
        project_id: r.project_id,
        project_key: r.project_key,
        name: r.name,
        kind: r.kind,
        description: r.description,
        key_version: r.key_version,
        created_by: r.created_by,
        last_rotated_at: r.last_rotated_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    })
}

async fn write_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
    user_id: Option<Uuid>,
    action: &str,
    headers: &HeaderMap,
    addr: SocketAddr,
) -> AppResult<()> {
    let ip = sqlx::types::ipnetwork::IpNetwork::from(addr.ip());
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(500).collect::<String>());
    sqlx::query(
        r#"
        INSERT INTO vault_audit_log (id, vault_item_id, user_id, action, ip, user_agent)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(item_id)
    .bind(user_id)
    .bind(action)
    .bind(ip)
    .bind(ua)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Redis token bucket: per-user counter that expires after the window.
/// Cheap and good enough for "10 reveals per hour".
async fn rate_limit_ok(state: &AppState, user_id: Uuid) -> AppResult<bool> {
    let key = format!("sprintly:vault:reveal:{user_id}");
    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("redis: {e}")))?;
    let n: i64 = redis::cmd("INCR")
        .arg(&key)
        .query_async(&mut conn)
        .await?;
    if n == 1 {
        // First call in window → set TTL.
        let _: () = redis::cmd("EXPIRE")
            .arg(&key)
            .arg(3600)
            .query_async(&mut conn)
            .await?;
    }
    Ok((n as u32) <= REVEAL_LIMIT_PER_HOUR)
}
