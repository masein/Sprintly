//! Cross-cutting reads for the cmd-K palette + /me/tasks.
//!
//!   GET /search?q=...&limit=...  — task / project / user hits in one shot.
//!   GET /me/tasks                — tasks where the caller is the assignee.
//!
//! Search ranking:
//!   • tasks  — tsvector rank against `plainto_tsquery(q)`. Tie-break by
//!              trigram similarity of `title` against `q` for typo tolerance.
//!   • users  — trigram similarity on `handle` + `display_name`.
//!   • projects — exact key match wins, then name trigram.
//!
//! Always scoped by `accessible_project_ids` so non-members never see hits
//! they shouldn't.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::permissions::Role as GlobalRole,
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search))
        .route("/me/tasks", get(my_tasks))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SearchTaskHit {
    pub key: String,
    pub project_key: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub r#type: String,
}

#[derive(Debug, Serialize)]
pub struct SearchProjectHit {
    pub key: String,
    pub name: String,
    pub color: String,
    pub icon: String,
}

#[derive(Debug, Serialize)]
pub struct SearchUserHit {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
}

#[derive(Debug, Serialize)]
pub struct MyTaskItem {
    pub key: String,
    pub project_key: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub r#type: String,
    pub due_date: Option<NaiveDate>,
    pub updated_at: DateTime<Utc>,
}

async fn search(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<impl IntoResponse> {
    let raw = q.q.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("q required".into()));
    }
    if raw.len() > 100 {
        return Err(AppError::BadRequest("q too long".into()));
    }
    let limit = q.limit.unwrap_or(8).clamp(1, 25);

    let accessible = accessible_project_ids(&state.db, &user).await?;

    let tasks = if accessible.is_empty() {
        Vec::new()
    } else {
        // tsvector first; ties broken by trigram similarity so typos still
        // surface something. Coalesce because tsvector rank against an empty
        // query is NULL.
        sqlx::query!(
            r#"
            SELECT t.key             AS "key!: String",
                   p.key             AS "project_key!: String",
                   t.title           AS "title!: String",
                   t.status          AS "status!: String",
                   t.priority        AS "priority!: String",
                   t.type            AS "type!: String"
            FROM   tasks t
            JOIN   projects p ON p.id = t.project_id
            WHERE  t.project_id = ANY($1)
              AND  t.deleted_at IS NULL
              AND  (t.search_tsv @@ plainto_tsquery('english', $2)
                    OR t.title % $2)
            ORDER BY
                COALESCE(ts_rank(t.search_tsv, plainto_tsquery('english', $2)), 0) DESC,
                similarity(t.title, $2) DESC,
                t.updated_at DESC
            LIMIT $3
            "#,
            &accessible,
            raw,
            limit
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| SearchTaskHit {
            key: r.key,
            project_key: r.project_key,
            title: r.title,
            status: r.status,
            priority: r.priority,
            r#type: r.r#type,
        })
        .collect()
    };

    let projects: Vec<SearchProjectHit> = if accessible.is_empty() {
        Vec::new()
    } else {
        sqlx::query!(
            r#"
            SELECT key   AS "key!: String",
                   name  AS "name!: String",
                   color AS "color!: String",
                   icon  AS "icon!: String"
            FROM   projects
            WHERE  id = ANY($1) AND deleted_at IS NULL
              AND (key ILIKE $2 || '%' OR name % $2)
            ORDER BY (key ILIKE $2 || '%') DESC, similarity(name, $2) DESC
            LIMIT  $3
            "#,
            &accessible,
            raw,
            limit
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| SearchProjectHit {
            key: r.key,
            name: r.name,
            color: r.color,
            icon: r.icon,
        })
        .collect()
    };

    let users: Vec<SearchUserHit> = sqlx::query!(
        r#"
        SELECT id           AS "id!: Uuid",
               handle       AS "handle!: String",
               display_name AS "display_name!: String"
        FROM   users
        WHERE  deleted_at IS NULL
          AND  (handle ILIKE $1 || '%' OR display_name % $1)
        ORDER BY (handle ILIKE $1 || '%') DESC, similarity(display_name, $1) DESC
        LIMIT  $2
        "#,
        raw,
        limit
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| SearchUserHit {
        id: r.id,
        handle: r.handle,
        display_name: r.display_name,
    })
    .collect();

    Ok(Json(serde_json::json!({
        "tasks": tasks,
        "projects": projects,
        "users": users,
    })))
}

async fn my_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let rows = sqlx::query!(
        r#"
        SELECT t.key             AS "key!: String",
               p.key             AS "project_key!: String",
               t.title           AS "title!: String",
               t.status          AS "status!: String",
               t.priority        AS "priority!: String",
               t.type            AS "type!: String",
               t.due_date,
               t.updated_at      AS "updated_at!: DateTime<Utc>"
        FROM   tasks t
        JOIN   projects p ON p.id = t.project_id
        WHERE  t.assignee_id = $1
          AND  t.deleted_at IS NULL
          AND  p.deleted_at IS NULL
        ORDER BY
            CASE t.status
                WHEN 'in_progress' THEN 0
                WHEN 'review'      THEN 1
                WHEN 'todo'        THEN 2
                WHEN 'done'        THEN 3
                ELSE 4
            END,
            t.priority ASC,
            t.due_date NULLS LAST,
            t.updated_at DESC
        LIMIT 200
        "#,
        user.id
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<MyTaskItem> = rows
        .into_iter()
        .map(|r| MyTaskItem {
            key: r.key,
            project_key: r.project_key,
            title: r.title,
            status: r.status,
            priority: r.priority,
            r#type: r.r#type,
            due_date: r.due_date,
            updated_at: r.updated_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn accessible_project_ids(db: &PgPool, user: &CurrentUser) -> AppResult<Vec<Uuid>> {
    if user.role == GlobalRole::Admin {
        Ok(sqlx::query_scalar(
            r#"SELECT id FROM projects WHERE deleted_at IS NULL"#,
        )
        .fetch_all(db)
        .await?)
    } else {
        Ok(sqlx::query_scalar(
            r#"
            SELECT pm.project_id
            FROM   project_members pm
            JOIN   projects p ON p.id = pm.project_id
            WHERE  pm.user_id = $1 AND p.deleted_at IS NULL
            "#,
        )
        .bind(user.id)
        .fetch_all(db)
        .await?)
    }
}
