//! Users endpoints.
//!
//! M1 ships:
//!   GET   /users/me           — current user (exercises CurrentUser)
//!   PATCH /users/me           — self-edit (display_name, timezone, settings)
//!
//! Full CRUD (invite, suspend, delete, list, admin-edit others) lands as part
//! of the admin panel in M10. The handler shape here is the template
//! everything else follows.

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::permissions::{can, Action, Resource},
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/users/me", get(get_me).patch(patch_me))
}

#[derive(Debug, Serialize)]
pub struct MeDto {
    pub id: Uuid,
    pub email: String,
    pub handle: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub status: String,
    pub timezone: String,
    pub currency: String,
    pub settings: JsonValue,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn get_me(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    if !can(&user.as_actor(), Action::ViewUser, Resource::SelfRef) {
        return Err(AppError::Forbidden);
    }

    let row = sqlx::query!(
        r#"
        SELECT id           AS "id!: Uuid",
               email        AS "email!: String",
               handle       AS "handle!: String",
               display_name AS "display_name!: String",
               avatar_url,
               role         AS "role!: String",
               status       AS "status!: String",
               timezone     AS "timezone!: String",
               currency     AS "currency!: String",
               settings     AS "settings!: JsonValue",
               created_at   AS "created_at!: chrono::DateTime<chrono::Utc>",
               last_seen_at
        FROM   users
        WHERE  id = $1 AND deleted_at IS NULL
        "#,
        user.id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    Ok(Json(MeDto {
        id: row.id,
        email: row.email,
        handle: row.handle,
        display_name: row.display_name,
        avatar_url: row.avatar_url,
        role: row.role,
        status: row.status,
        timezone: row.timezone,
        currency: row.currency,
        settings: row.settings,
        created_at: row.created_at,
        last_seen_at: row.last_seen_at,
    }))
}

#[derive(Debug, Deserialize, Validate)]
pub struct PatchMeReq {
    #[validate(length(min = 1, max = 80))]
    pub display_name: Option<String>,
    #[validate(length(min = 1, max = 64))]
    pub timezone: Option<String>,
    pub settings: Option<JsonValue>,
}

async fn patch_me(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<PatchMeReq>,
) -> AppResult<impl IntoResponse> {
    if !can(&user.as_actor(), Action::EditOwnProfile, Resource::SelfRef) {
        return Err(AppError::Forbidden);
    }
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    // COALESCE — only touch fields the caller actually sent. Each Option is
    // explicit in the bind so we don't churn unrelated columns.
    sqlx::query(
        r#"
        UPDATE users SET
            display_name = COALESCE($2, display_name),
            timezone     = COALESCE($3, timezone),
            settings     = COALESCE($4, settings)
        WHERE id = $1
        "#,
    )
    .bind(user.id)
    .bind(req.display_name)
    .bind(req.timezone)
    .bind(req.settings)
    .execute(&state.db)
    .await?;

    // Return the freshly-updated row.
    get_me(State(state), user).await
}
