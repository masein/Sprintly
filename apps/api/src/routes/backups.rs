//! Backup endpoints.
//!
//!   POST /admin/backups   — enqueue a 'create_backup' job, return the row.
//!   GET  /admin/backups   — list rows (most recent first).
//!
//! The actual `pg_dump` invocation lives in the jobs worker so the request
//! returns immediately. Worker shells out to `pg_dump`, writes to a temp
//! file, then PUTs the file into MinIO under `backups/YYYY-MM-DD/<id>.dump`.
//! On success the row gets `status='done'`, size, storage_key. On failure,
//! `error` is set.
//!
//! Restore is a *documented* manual procedure — there's intentionally no
//! one-click restore in the UI. See `docs/SECURITY.md` for the runbook.

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::{
    domain::permissions::Role as GlobalRole,
    infra::AppState,
    middleware::CurrentUser,
    routes::admin_panel::write_admin_audit,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/backups", post(start_backup).get(list_backups))
}

#[derive(Debug, Serialize)]
pub struct BackupRow {
    pub id: Uuid,
    pub status: String,
    pub requested_by: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub size_bytes: Option<i64>,
    pub storage_key: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

async fn start_backup(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }

    let backup_id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"INSERT INTO backups (id, requested_by, status) VALUES ($1, $2, 'pending')"#,
    )
    .bind(backup_id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    // Enqueue the worker job carrying the backup row id.
    sqlx::query(
        r#"
        INSERT INTO jobs (id, kind, payload, run_at)
        VALUES ($1, 'create_backup', jsonb_build_object('backup_id', $2::text), now())
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(backup_id)
    .execute(&mut *tx)
    .await?;
    write_admin_audit(
        &mut tx,
        user.id,
        "backup.start",
        None,
        &serde_json::json!({ "backup_id": backup_id }),
        &headers,
        ConnectInfo(addr),
    )
    .await?;
    tx.commit().await?;

    let row = fetch(&state.db, backup_id).await?;
    Ok((StatusCode::ACCEPTED, Json(row)))
}

async fn list_backups(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT id            AS "id!: Uuid",
               status        AS "status!: String",
               requested_by,
               started_at,
               finished_at,
               size_bytes,
               storage_key,
               error,
               created_at    AS "created_at!: DateTime<Utc>"
        FROM   backups
        ORDER  BY created_at DESC
        LIMIT  100
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<BackupRow> = rows
        .into_iter()
        .map(|r| BackupRow {
            id: r.id,
            status: r.status,
            requested_by: r.requested_by,
            started_at: r.started_at,
            finished_at: r.finished_at,
            size_bytes: r.size_bytes,
            storage_key: r.storage_key,
            error: r.error,
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn fetch(db: &sqlx::PgPool, id: Uuid) -> AppResult<BackupRow> {
    let r = sqlx::query!(
        r#"
        SELECT id            AS "id!: Uuid",
               status        AS "status!: String",
               requested_by,
               started_at,
               finished_at,
               size_bytes,
               storage_key,
               error,
               created_at    AS "created_at!: DateTime<Utc>"
        FROM   backups WHERE id = $1
        "#,
        id
    )
    .fetch_one(db)
    .await?;
    Ok(BackupRow {
        id: r.id,
        status: r.status,
        requested_by: r.requested_by,
        started_at: r.started_at,
        finished_at: r.finished_at,
        size_bytes: r.size_bytes,
        storage_key: r.storage_key,
        error: r.error,
        created_at: r.created_at,
    })
}
