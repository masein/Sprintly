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
    Router::new()
        .route("/users/me", get(get_me).patch(patch_me))
        .route("/users/me/avatar", axum::routing::put(put_avatar))
}

/// Generated-avatar styles the API will accept. Kept in lock-step with the
/// `users_avatar_style_chk` constraint and the web `lib/avatar` generators.
const AVATAR_STYLES: [&str; 4] = ["beaver", "robot", "identicon", "glyph"];

#[derive(Debug, Serialize)]
pub struct MeDto {
    pub id: Uuid,
    pub email: String,
    pub handle: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub avatar_style: Option<String>,
    pub avatar_seed: Option<String>,
    pub role: String,
    pub status: String,
    pub timezone: String,
    pub currency: String,
    pub settings: JsonValue,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn get_me(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
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
               avatar_style,
               avatar_seed,
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
        avatar_style: row.avatar_style,
        avatar_seed: row.avatar_seed,
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

/// The whole avatar is one cohesive setting, so it gets its own endpoint that
/// *replaces* all three fields unconditionally (a COALESCE patch can't express
/// "clear back to the generated default"). Sending all-null reverts a user to
/// the deterministic generated avatar.
#[derive(Debug, Deserialize)]
pub struct AvatarReq {
    /// Uploaded/linked image. Only `data:` and `https:` are accepted so a
    /// stored value can never smuggle a `javascript:`/`http:` URL into an
    /// `<img src>`. A data URL keeps self-hosted installs offline.
    pub url: Option<String>,
    pub style: Option<String>,
    pub seed: Option<String>,
}

// ~512 KiB — a generously-sized data URL for a downscaled avatar, with room to
// spare. Bigger than this is almost certainly a full-res photo we don't want.
const AVATAR_URL_MAX: usize = 512 * 1024;

async fn put_avatar(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<AvatarReq>,
) -> AppResult<impl IntoResponse> {
    if !can(&user.as_actor(), Action::EditOwnProfile, Resource::SelfRef) {
        return Err(AppError::Forbidden);
    }

    if let Some(url) = req.url.as_deref() {
        if url.len() > AVATAR_URL_MAX {
            return Err(AppError::Validation("avatar image is too large".into()));
        }
        if !(url.starts_with("data:image/") || url.starts_with("https://")) {
            return Err(AppError::Validation(
                "avatar url must be a data:image/ or https:// URL".into(),
            ));
        }
    }
    if let Some(style) = req.style.as_deref() {
        if !AVATAR_STYLES.contains(&style) {
            return Err(AppError::Validation("unknown avatar style".into()));
        }
    }
    if let Some(seed) = req.seed.as_deref() {
        if seed.len() > 64 {
            return Err(AppError::Validation("avatar seed is too long".into()));
        }
    }

    sqlx::query(
        r#"
        UPDATE users SET
            avatar_url   = $2,
            avatar_style = $3,
            avatar_seed  = $4
        WHERE id = $1
        "#,
    )
    .bind(user.id)
    .bind(req.url)
    .bind(req.style)
    .bind(req.seed)
    .execute(&state.db)
    .await?;

    get_me(State(state), user).await
}
