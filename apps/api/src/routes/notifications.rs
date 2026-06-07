//! `/me/notifications` — the in-app notification center.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{infra::AppState, middleware::CurrentUser, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/notifications", get(list))
        .route("/me/notifications/unread-count", get(unread_count))
        .route("/me/notifications/read-all", post(read_all))
        .route("/me/notifications/:id/read", post(read_one))
}

#[derive(Debug, Serialize, FromRow)]
pub struct NotificationDto {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
    pub actor_handle: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

async fn list(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    let rows: Vec<NotificationDto> = sqlx::query_as(
        r#"
        SELECT n.id, n.kind, n.title, n.body, n.link,
               u.handle AS actor_handle, n.read_at, n.created_at
        FROM   notifications n
        LEFT JOIN users u ON u.id = n.actor_id
        WHERE  n.user_id = $1
        ORDER  BY n.created_at DESC
        LIMIT  50
        "#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn unread_count(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL"#,
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "count": count })))
}

async fn read_one(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    sqlx::query(
        r#"UPDATE notifications SET read_at = now()
           WHERE id = $1 AND user_id = $2 AND read_at IS NULL"#,
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn read_all(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    sqlx::query(
        r#"UPDATE notifications SET read_at = now()
           WHERE user_id = $1 AND read_at IS NULL"#,
    )
    .bind(user.id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
