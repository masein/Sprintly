//! Achievement endpoints.
//!
//!   GET  /achievements              — catalog (everyone can view)
//!   GET  /me/achievements           — what *I* have earned
//!   POST /me/achievements/rtfm      — RTFM trigger (single user-driven award)

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{infra::AppState, middleware::CurrentUser, AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/achievements", get(catalog))
        .route("/me/achievements", get(my_achievements))
        .route("/me/achievements/rtfm", post(rtfm))
}

#[derive(Debug, Serialize)]
pub struct CatalogRow {
    pub code: String,
    pub title: String,
    pub description: String,
    pub icon: String,
}

#[derive(Debug, Serialize)]
pub struct AwardedRow {
    pub code: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub awarded_at: DateTime<Utc>,
    pub context: serde_json::Value,
}

async fn catalog(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let rows = sqlx::query!(
        r#"
        SELECT code        AS "code!: String",
               title       AS "title!: String",
               description AS "description!: String",
               icon        AS "icon!: String"
        FROM   achievements
        ORDER  BY title ASC
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<CatalogRow> = rows
        .into_iter()
        .map(|r| CatalogRow {
            code: r.code,
            title: r.title,
            description: r.description,
            icon: r.icon,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn my_achievements(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let rows = sqlx::query!(
        r#"
        SELECT a.code         AS "code!: String",
               a.title        AS "title!: String",
               a.description  AS "description!: String",
               a.icon         AS "icon!: String",
               ua.awarded_at  AS "awarded_at!: DateTime<Utc>",
               ua.context     AS "context!: serde_json::Value"
        FROM   user_achievements ua
        JOIN   achievements a ON a.id = ua.achievement_id
        WHERE  ua.user_id = $1
        ORDER  BY ua.awarded_at DESC
        "#,
        user.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<AwardedRow> = rows
        .into_iter()
        .map(|r| AwardedRow {
            code: r.code,
            title: r.title,
            description: r.description,
            icon: r.icon,
            awarded_at: r.awarded_at,
            context: r.context,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn rtfm(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    let aid: Option<Uuid> = sqlx::query_scalar("SELECT id FROM achievements WHERE code = 'RTFM'")
        .fetch_optional(&state.db)
        .await?;
    let Some(aid) = aid else {
        return Err(AppError::Internal(anyhow::anyhow!(
            "RTFM achievement missing from catalog"
        )));
    };
    let r = sqlx::query(
        r#"
        INSERT INTO user_achievements (user_id, achievement_id, context)
        VALUES ($1, $2, '{"trigger":"docs_visit"}'::jsonb)
        ON CONFLICT (user_id, achievement_id) DO NOTHING
        "#,
    )
    .bind(user.id)
    .bind(aid)
    .execute(&state.db)
    .await?;
    // 201 on first award, 200 on idempotent replay.
    if r.rows_affected() > 0 {
        Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({ "awarded": true })),
        ))
    } else {
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "awarded": false })),
        ))
    }
}
