//! Admin panel endpoints.
//!
//! Strictly global-admin gated. Every write writes a row to admin_audit_log.
//! Reads:
//!   GET    /admin/users                     — list + filters
//!   GET    /admin/audit                     — admin audit feed
//!   GET    /admin/health                    — DB / Redis / MinIO traffic lights
//!
//! Writes:
//!   POST   /admin/users/:id/suspend         — sets status='suspended'
//!   POST   /admin/users/:id/reactivate      — sets status='active'
//!   POST   /admin/users/:id/role            — body: {"role":"admin|member|viewer"}
//!   POST   /admin/users/:id/reset-password  — generates a single-use reset token,
//!                                              returns the URL (no email yet)
//!
//! Backups + webhooks live in their own modules. They're admin-scoped too,
//! routed alongside.

use std::net::IpAddr;

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::{
    domain::permissions::Role as GlobalRole,
    infra::AppState,
    middleware::{client_ip, CurrentUser},
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list_users))
        .route("/admin/users/:id/suspend", post(suspend))
        .route("/admin/users/:id/reactivate", post(reactivate))
        .route("/admin/users/:id/role", post(set_role))
        .route("/admin/users/:id/reset-password", post(reset_password))
        .route("/admin/audit", get(list_audit))
        .route("/admin/health", get(health))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminUserRow {
    pub id: Uuid,
    pub email: String,
    pub handle: String,
    pub display_name: String,
    pub role: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetRoleReq {
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct ResetPasswordResp {
    pub token: String,
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AuditRow {
    pub id: Uuid,
    pub actor_handle: Option<String>,
    pub action: String,
    pub target_handle: Option<String>,
    pub payload: serde_json::Value,
    pub ip: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct HealthDto {
    pub db: HealthCheck,
    pub redis: HealthCheck,
    pub minio: HealthCheck,
    pub version: String,
    pub jobs: JobsStat,
}

#[derive(Debug, Serialize)]
pub struct HealthCheck {
    pub ok: bool,
    pub latency_ms: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobsStat {
    pub pending: i64,
    pub running: i64,
    pub failed: i64,
    pub last_finished_at: Option<DateTime<Utc>>,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn list_users(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListUsersQuery>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let rows = sqlx::query!(
        r#"
        SELECT id            AS "id!: Uuid",
               email         AS "email!: String",
               handle        AS "handle!: String",
               display_name  AS "display_name!: String",
               role          AS "role!: String",
               status        AS "status!: String",
               created_at    AS "created_at!: DateTime<Utc>",
               last_seen_at
        FROM   users
        WHERE  deleted_at IS NULL
          AND  ($1::text IS NULL OR
                handle ILIKE '%' || $1 || '%'
                OR display_name ILIKE '%' || $1 || '%'
                OR email::text ILIKE '%' || $1 || '%')
          AND  ($2::text IS NULL OR status = $2)
          AND  ($3::text IS NULL OR role = $3)
        ORDER  BY handle
        LIMIT  500
        "#,
        q.q.as_deref(),
        q.status.as_deref(),
        q.role.as_deref(),
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<AdminUserRow> = rows
        .into_iter()
        .map(|r| AdminUserRow {
            id: r.id,
            email: r.email,
            handle: r.handle,
            display_name: r.display_name,
            role: r.role,
            status: r.status,
            created_at: r.created_at,
            last_seen_at: r.last_seen_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn suspend(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    if id == user.id {
        return Err(AppError::Conflict("can't suspend yourself".into()));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"UPDATE users SET status = 'suspended' WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    // Revoke every active session so the suspended user gets kicked out now,
    // not at access-token expiry.
    sqlx::query(
        r#"UPDATE sessions SET revoked_at = now(), revoked_reason = 'suspended'
           WHERE user_id = $1 AND revoked_at IS NULL"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    write_admin_audit(
        &mut tx,
        user.id,
        "user.suspend",
        Some(id),
        &serde_json::json!({}),
        &headers,
        ConnectInfo(addr),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reactivate(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"UPDATE users SET status = 'active' WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    write_admin_audit(
        &mut tx,
        user.id,
        "user.reactivate",
        Some(id),
        &serde_json::json!({}),
        &headers,
        ConnectInfo(addr),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_role(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
    Json(req): Json<SetRoleReq>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    if !matches!(req.role.as_str(), "admin" | "member" | "viewer") {
        return Err(AppError::BadRequest("role must be admin/member/viewer".into()));
    }
    if id == user.id && req.role != "admin" {
        return Err(AppError::Conflict(
            "can't demote yourself — ask another admin".into(),
        ));
    }

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"UPDATE users SET role = $2 WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .bind(&req.role)
    .execute(&mut *tx)
    .await?;
    write_admin_audit(
        &mut tx,
        user.id,
        "user.role",
        Some(id),
        &serde_json::json!({ "new_role": req.role }),
        &headers,
        ConnectInfo(addr),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reset_password(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    // Mint a single-use reset token (30 min). Hash on disk; return plaintext.
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    let hash: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(raw);
        h.finalize().into()
    };
    let expires_at = Utc::now() + Duration::minutes(30);

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO password_reset_tokens (id, user_id, token_hash, expires_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(hash.as_slice())
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    write_admin_audit(
        &mut tx,
        user.id,
        "user.reset_password",
        Some(id),
        &serde_json::json!({}),
        &headers,
        ConnectInfo(addr),
    )
    .await?;
    tx.commit().await?;

    let url = format!(
        "{base}/login?reset={token}",
        base = state.cfg.public_url.trim_end_matches('/'),
    );
    Ok(Json(ResetPasswordResp {
        token,
        url,
        expires_at,
    }))
}

async fn list_audit(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query!(
        r#"
        SELECT a.id           AS "id!: Uuid",
               actor.handle   AS "actor_handle?: String",
               a.action       AS "action!: String",
               target.handle  AS "target_handle?: String",
               a.payload      AS "payload!: serde_json::Value",
               a.ip,
               a.occurred_at  AS "occurred_at!: DateTime<Utc>"
        FROM   admin_audit_log a
        LEFT JOIN users actor  ON actor.id  = a.actor_id
        LEFT JOIN users target ON target.id = a.target_user_id
        ORDER  BY a.occurred_at DESC
        LIMIT  $1
        "#,
        limit
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<AuditRow> = rows
        .into_iter()
        .map(|r| AuditRow {
            id: r.id,
            actor_handle: r.actor_handle,
            action: r.action,
            target_handle: r.target_handle,
            payload: r.payload,
            ip: r.ip.map(|i| i.to_string()),
            occurred_at: r.occurred_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn health(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;

    let db_check = {
        let start = std::time::Instant::now();
        let r = sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&state.db)
            .await;
        HealthCheck {
            ok: r.is_ok(),
            latency_ms: start.elapsed().as_millis() as u64,
            detail: r.err().map(|e| e.to_string()),
        }
    };

    let redis_check = {
        let start = std::time::Instant::now();
        let r = match state.redis.get().await {
            Ok(mut conn) => redis::cmd("PING")
                .query_async::<_, String>(&mut conn)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        HealthCheck {
            ok: r.as_ref().map(|s| s == "PONG").unwrap_or(false),
            latency_ms: start.elapsed().as_millis() as u64,
            detail: r.err(),
        }
    };

    let minio_check = {
        // No SDK; just probe the public endpoint's /minio/health/live.
        let start = std::time::Instant::now();
        let url = format!(
            "{}/minio/health/live",
            state.cfg.minio.endpoint.trim_end_matches('/'),
        );
        let r = tokio::process::Command::new("wget")
            .args(["-qO-", "--tries=1", "--timeout=2", &url])
            .output()
            .await;
        HealthCheck {
            ok: r.as_ref().map(|o| o.status.success()).unwrap_or(false),
            latency_ms: start.elapsed().as_millis() as u64,
            detail: r.err().map(|e| e.to_string()),
        }
    };

    let jobs_stat = {
        let row = sqlx::query!(
            r#"
            SELECT
              COUNT(*) FILTER (WHERE finished_at IS NULL AND claimed_at IS NULL)
                  AS "pending!: i64",
              COUNT(*) FILTER (WHERE finished_at IS NULL AND claimed_at IS NOT NULL)
                  AS "running!: i64",
              COUNT(*) FILTER (WHERE finished_at IS NOT NULL AND last_error IS NOT NULL)
                  AS "failed!: i64",
              MAX(finished_at) AS last_finished_at
            FROM jobs
            "#,
        )
        .fetch_one(&state.db)
        .await?;
        JobsStat {
            pending: row.pending,
            running: row.running,
            failed: row.failed,
            last_finished_at: row.last_finished_at,
        }
    };

    Ok(Json(HealthDto {
        db: db_check,
        redis: redis_check,
        minio: minio_check,
        version: env!("CARGO_PKG_VERSION").to_string(),
        jobs: jobs_stat,
    }))
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn require_admin(user: &CurrentUser) -> AppResult<()> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn write_admin_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor_id: Uuid,
    action: &str,
    target_user_id: Option<Uuid>,
    payload: &serde_json::Value,
    headers: &HeaderMap,
    addr: ConnectInfo<SocketAddr>,
) -> AppResult<()> {
    let ip: IpAddr = client_ip(headers, addr);
    let net = sqlx::types::ipnetwork::IpNetwork::from(ip);
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(500).collect::<String>());
    sqlx::query(
        r#"
        INSERT INTO admin_audit_log (id, actor_id, action, target_user_id, payload, ip, user_agent)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(actor_id)
    .bind(action)
    .bind(target_user_id)
    .bind(payload)
    .bind(net)
    .bind(ua)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
