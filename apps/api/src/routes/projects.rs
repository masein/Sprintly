//! Projects + members endpoints.
//!
//!   POST   /projects                          — create. Creator becomes lead.
//!                                                Auto-creates default board + 3 columns.
//!   GET    /projects                          — list. Admins see all; others see
//!                                                projects they're a member of.
//!   GET    /projects/:key                     — detail.
//!   PATCH  /projects/:key                     — edit (lead only).
//!   POST   /projects/:key/archive             — archive (lead only).
//!   POST   /projects/:key/unarchive           — restore (lead only).
//!
//!   GET    /projects/:key/members             — list members.
//!   POST   /projects/:key/members             — add (lead only).
//!   DELETE /projects/:key/members/:user_id    — remove (lead only; can't remove the last lead).
//!   PATCH  /projects/:key/members/:user_id    — change role (lead only).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action, ProjectRole, Resource, Role as GlobalRole},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", post(create).get(list))
        .route("/projects/:key", get(detail).patch(edit))
        .route("/projects/:key/archive", post(archive))
        .route("/projects/:key/unarchive", post(unarchive))
        .route("/projects/:key/members", get(list_members).post(add_member))
        .route(
            "/projects/:key/members/:user_id",
            delete(remove_member).patch(change_member_role),
        )
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Validate)]
pub struct CreateProjectReq {
    /// 2-10 chars, uppercase, starts with letter. Used in task IDs like `WEB-142`.
    pub key: String,
    #[validate(length(min = 1, max = 80))]
    pub name: String,
    #[validate(length(max = 4000))]
    pub description: Option<String>,
    #[validate(length(min = 1, max = 32))]
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditProjectReq {
    #[validate(length(min = 1, max = 80))]
    pub name: Option<String>,
    #[validate(length(max = 4000))]
    pub description: Option<String>,
    #[validate(length(min = 1, max = 32))]
    pub icon: Option<String>,
    pub color: Option<String>,
    pub settings: Option<serde_json::Value>,
}

fn validate_project_key(s: &str) -> AppResult<()> {
    let bytes = s.as_bytes();
    let ok = (2..=10).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if !ok {
        return Err(AppError::Validation(
            "project key must be 2-10 uppercase letters/digits starting with a letter".into(),
        ));
    }
    Ok(())
}

fn validate_hex_color(s: &str) -> AppResult<()> {
    let bytes = s.as_bytes();
    let ok =
        bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return Err(AppError::Validation("color must be #rrggbb".into()));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ProjectDto {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub color: String,
    pub archived_at: Option<DateTime<Utc>>,
    pub settings: serde_json::Value,
    pub member_count: i64,
    pub your_role: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct MemberDto {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberReq {
    pub user_id: Uuid,
    pub role: Option<String>, // defaults to "contributor"
}

#[derive(Debug, Deserialize)]
pub struct ChangeRoleReq {
    pub role: String,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateProjectReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    validate_project_key(&req.key)?;
    if let Some(c) = req.color.as_deref() {
        validate_hex_color(c)?;
    }

    if !can(&user.as_actor(), Action::CreateProject, Resource::SelfRef) {
        return Err(AppError::Forbidden);
    }

    let project_id = Uuid::now_v7();
    let board_id = Uuid::now_v7();
    let color = req.color.unwrap_or_else(|| "#7c5cff".into());
    let icon = req.icon.unwrap_or_else(|| "folder".into());

    let mut tx = state.db.begin().await?;

    let insert = sqlx::query(
        r#"
        INSERT INTO projects (id, key, name, description, icon, color, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(project_id)
    .bind(&req.key)
    .bind(&req.name)
    .bind(req.description.as_deref().unwrap_or(""))
    .bind(&icon)
    .bind(&color)
    .bind(user.id)
    .execute(&mut *tx)
    .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(AppError::Conflict(format!(
                "project key {} already exists",
                req.key
            )));
        }
        if let Some(check) = db_err.constraint() {
            if check == "projects_key_format" {
                return Err(AppError::Validation(
                    "project key must be uppercase, 2-10 chars".into(),
                ));
            }
        }
    }
    insert?;

    // Creator becomes lead.
    sqlx::query(
        r#"
        INSERT INTO project_members (project_id, user_id, role, added_by)
        VALUES ($1, $2, 'lead', $2)
        "#,
    )
    .bind(project_id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;

    // Default board.
    sqlx::query(
        r#"
        INSERT INTO boards (id, project_id, name, type, is_default)
        VALUES ($1, $2, 'Board', 'kanban', true)
        "#,
    )
    .bind(board_id)
    .bind(project_id)
    .execute(&mut *tx)
    .await?;

    // Default columns: To do / In progress / Done. Spaced out so reorder/insert is easy.
    let cols: [(&str, &str, f64); 3] = [
        ("To do", "todo", 1024.0),
        ("In progress", "in_progress", 2048.0),
        ("Done", "done", 3072.0),
    ];
    for (name, category, sort_order) in cols {
        sqlx::query(
            r#"
            INSERT INTO board_columns (id, board_id, name, category, sort_order)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(board_id)
        .bind(name)
        .bind(category)
        .bind(sort_order)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let dto = fetch_project_dto(&state.db, project_id, user.id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn list(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    // Admins see everything; everyone else sees what they're a member of.
    let is_admin = user.role == GlobalRole::Admin;

    let rows = sqlx::query!(
        r#"
        SELECT  p.id           AS "id!: Uuid",
                p.key          AS "key!: String",
                p.name         AS "name!: String",
                p.description  AS "description!: String",
                p.icon         AS "icon!: String",
                p.color        AS "color!: String",
                p.archived_at,
                p.settings     AS "settings!: serde_json::Value",
                p.created_at   AS "created_at!: DateTime<Utc>",
                pm_self.role   AS "your_role?: String",
                (SELECT COUNT(*) FROM project_members pmc WHERE pmc.project_id = p.id)
                               AS "member_count!: i64"
        FROM    projects p
        LEFT JOIN project_members pm_self
               ON pm_self.project_id = p.id AND pm_self.user_id = $1
        WHERE   p.deleted_at IS NULL
          AND   ($2 OR pm_self.user_id IS NOT NULL)
        ORDER BY p.archived_at IS NOT NULL, p.created_at DESC
        "#,
        user.id,
        is_admin
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<ProjectDto> = rows
        .into_iter()
        .map(|r| ProjectDto {
            id: r.id,
            key: r.key,
            name: r.name,
            description: r.description,
            icon: r.icon,
            color: r.color,
            archived_at: r.archived_at,
            settings: r.settings,
            member_count: r.member_count,
            your_role: r.your_role,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items })))
}

async fn detail(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let dto = fetch_project_dto(&state.db, ctx.id, user.id).await?;
    Ok(Json(dto))
}

async fn edit(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(req): Json<EditProjectReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if let Some(c) = req.color.as_deref() {
        validate_hex_color(c)?;
    }
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        UPDATE projects SET
            name        = COALESCE($2, name),
            description = COALESCE($3, description),
            icon        = COALESCE($4, icon),
            color       = COALESCE($5, color),
            settings    = COALESCE($6, settings)
        WHERE id = $1
        "#,
    )
    .bind(ctx.id)
    .bind(req.name)
    .bind(req.description)
    .bind(req.icon)
    .bind(req.color)
    .bind(req.settings)
    .execute(&state.db)
    .await?;

    let dto = fetch_project_dto(&state.db, ctx.id, user.id).await?;
    Ok(Json(dto))
}

async fn archive(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ArchiveProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE projects SET archived_at = now() WHERE id = $1 AND archived_at IS NULL")
        .bind(ctx.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unarchive(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ArchiveProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE projects SET archived_at = NULL WHERE id = $1")
        .bind(ctx.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_members(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT pm.user_id      AS "user_id!: Uuid",
               u.handle        AS "handle!: String",
               u.display_name  AS "display_name!: String",
               u.avatar_url,
               pm.role         AS "role!: String",
               pm.added_at     AS "added_at!: DateTime<Utc>"
        FROM   project_members pm
        JOIN   users u ON u.id = pm.user_id
        WHERE  pm.project_id = $1 AND u.deleted_at IS NULL
        ORDER BY pm.role, u.handle
        "#,
        ctx.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<MemberDto> = rows
        .into_iter()
        .map(|r| MemberDto {
            user_id: r.user_id,
            handle: r.handle,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            role: r.role,
            added_at: r.added_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn add_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(req): Json<AddMemberReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(
        &user.as_actor(),
        Action::AddProjectMember,
        ctx.as_resource(),
    ) {
        return Err(AppError::Forbidden);
    }
    let role = req.role.as_deref().unwrap_or("contributor");
    if ProjectRole::parse(role).is_none() {
        return Err(AppError::BadRequest(
            "role must be lead/contributor/watcher".into(),
        ));
    }

    // Confirm the target user exists & isn't soft-deleted.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(req.user_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    sqlx::query(
        r#"
        INSERT INTO project_members (project_id, user_id, role, added_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role
        "#,
    )
    .bind(ctx.id)
    .bind(req.user_id)
    .bind(role)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((key, target)): Path<(String, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(
        &user.as_actor(),
        Action::RemoveProjectMember,
        ctx.as_resource(),
    ) {
        return Err(AppError::Forbidden);
    }
    ensure_not_last_lead(&state.db, ctx.id, target).await?;
    sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2")
        .bind(ctx.id)
        .bind(target)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn change_member_role(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((key, target)): Path<(String, Uuid)>,
    Json(req): Json<ChangeRoleReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(
        &user.as_actor(),
        Action::ChangeProjectMemberRole,
        ctx.as_resource(),
    ) {
        return Err(AppError::Forbidden);
    }
    if ProjectRole::parse(&req.role).is_none() {
        return Err(AppError::BadRequest(
            "role must be lead/contributor/watcher".into(),
        ));
    }
    if req.role != "lead" {
        ensure_not_last_lead(&state.db, ctx.id, target).await?;
    }
    sqlx::query(
        r#"
        UPDATE project_members SET role = $3
         WHERE project_id = $1 AND user_id = $2
        "#,
    )
    .bind(ctx.id)
    .bind(target)
    .bind(&req.role)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── helpers ────────────────────────────────────────────────────────────────

async fn ensure_not_last_lead(db: &PgPool, project_id: Uuid, target: Uuid) -> AppResult<()> {
    // Only block if the target IS currently a lead and they're the only one.
    let n: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM   project_members
        WHERE  project_id = $1 AND role = 'lead'
        "#,
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;

    let target_is_lead: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(SELECT 1 FROM project_members
                       WHERE project_id = $1 AND user_id = $2 AND role = 'lead')
        "#,
    )
    .bind(project_id)
    .bind(target)
    .fetch_one(db)
    .await?;

    if target_is_lead && n <= 1 {
        return Err(AppError::Conflict(
            "can't remove the only lead — promote someone else first".into(),
        ));
    }
    Ok(())
}

async fn fetch_project_dto(db: &PgPool, project_id: Uuid, user_id: Uuid) -> AppResult<ProjectDto> {
    let r = sqlx::query!(
        r#"
        SELECT  p.id           AS "id!: Uuid",
                p.key          AS "key!: String",
                p.name         AS "name!: String",
                p.description  AS "description!: String",
                p.icon         AS "icon!: String",
                p.color        AS "color!: String",
                p.archived_at,
                p.settings     AS "settings!: serde_json::Value",
                p.created_at   AS "created_at!: DateTime<Utc>",
                pm.role        AS "your_role?: String",
                (SELECT COUNT(*) FROM project_members pmc WHERE pmc.project_id = p.id)
                               AS "member_count!: i64"
        FROM    projects p
        LEFT JOIN project_members pm
               ON pm.project_id = p.id AND pm.user_id = $2
        WHERE   p.id = $1
        "#,
        project_id,
        user_id
    )
    .fetch_one(db)
    .await?;
    Ok(ProjectDto {
        id: r.id,
        key: r.key,
        name: r.name,
        description: r.description,
        icon: r.icon,
        color: r.color,
        archived_at: r.archived_at,
        settings: r.settings,
        member_count: r.member_count,
        your_role: r.your_role,
        created_at: r.created_at,
    })
}
