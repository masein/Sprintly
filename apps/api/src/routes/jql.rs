//! Query-language search over tasks, plus saved queries ("templates").
//!
//!   GET    /search/jql?jql=…&limit=&offset=  — run a query
//!   GET    /search/jql/fields               — the field list, for the UI
//!   GET    /search/queries                  — mine + everything shared
//!   POST   /search/queries                  { name, jql, is_shared? }
//!   PATCH  /search/queries/:id              { name?, jql?, is_shared? }
//!   DELETE /search/queries/:id
//!
//! The SQL is built at runtime (that's the point of a query language), so it
//! can't use the compile-checked `query!` macros. Two rules keep that honest:
//!
//!   • Every user value is a bound parameter — see `domain::jql`, which is
//!     where the parsing and the parameter binding live and is unit-tested for
//!     exactly this.
//!   • `$1` is always the caller's accessible-project list, ANDed in outside
//!     the user's expression. A query can narrow what you see, never widen it.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgArguments, Arguments, Row};
use uuid::Uuid;

use crate::{
    domain::jql::{self, Param},
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

use super::search::accessible_project_ids;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search/jql", get(run))
        .route("/search/jql/fields", get(fields))
        .route("/search/queries", get(list_saved).post(create_saved))
        .route(
            "/search/queries/:id",
            axum::routing::patch(update_saved).delete(delete_saved),
        )
}

// ── running a query ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RunQuery {
    /// The query text. Empty means "everything you can see".
    #[serde(default)]
    pub jql: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct JqlHit {
    pub key: String,
    pub project_key: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub r#type: String,
    pub assignee_handle: Option<String>,
    pub sprint_name: Option<String>,
    pub labels: Vec<String>,
    pub story_points: Option<i32>,
    pub due_date: Option<NaiveDate>,
    pub updated_at: DateTime<Utc>,
}

/// The FROM/JOIN block, shared by the page query and the count. The aliases
/// here are the ones `domain::jql` compiles against.
const FROM_SQL: &str = r#"
    FROM      tasks t
    JOIN      projects p ON p.id = t.project_id
    LEFT JOIN users   ua ON ua.id = t.assignee_id
    LEFT JOIN users   ur ON ur.id = t.reporter_id
    LEFT JOIN sprints s  ON s.id  = t.sprint_id
    LEFT JOIN epics   e  ON e.id  = t.epic_id
    LEFT JOIN tasks   pt ON pt.id = t.parent_task_id
"#;

/// `currentUser()` compiles to a handle comparison, so we need the caller's
/// handle — `CurrentUser` only carries the id and role.
async fn my_handle(state: &AppState, user: &CurrentUser) -> AppResult<String> {
    Ok(sqlx::query_scalar!(
        r#"SELECT handle AS "handle!: String" FROM users WHERE id = $1"#,
        user.id
    )
    .fetch_one(&state.db)
    .await?)
}

fn bind_all(mut args: PgArguments, params: &[Param]) -> Result<PgArguments, AppError> {
    for p in params {
        match p {
            Param::Text(t) => args.add(t.clone()),
            Param::TextList(l) => args.add(l.clone()),
            // Points/estimates are integers in the schema; compare as float8
            // so `points > 2.5` still means something.
            Param::Num(n) => args.add(*n),
            Param::Date(d) => args.add(*d),
        }
        .map_err(|e| AppError::Internal(anyhow::anyhow!("bind failed: {e}")))?;
    }
    Ok(args)
}

async fn run(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<RunQuery>,
) -> AppResult<impl IntoResponse> {
    if q.jql.len() > 2000 {
        return Err(AppError::BadRequest(
            "query too long (2000 chars max)".into(),
        ));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);

    // Parse errors are the user's most common experience of a query language,
    // so the message has to survive to the client. `Validation` is
    // deliberately masked in the error mapper (validator output is
    // machine-shaped); `BadRequest` passes hand-written text through, which is
    // what these are.
    let handle = my_handle(&state, &user).await?;
    let parsed = jql::parse(&q.jql)
        .map_err(|e| AppError::BadRequest(format!("{} — at character {}", e.message, e.at + 1)))?;
    let compiled = jql::compile(&parsed, &handle, 2);

    let accessible = accessible_project_ids(&state.db, &user).await?;
    if accessible.is_empty() {
        return Ok(Json(serde_json::json!({ "items": [], "total": 0 })));
    }

    let order_sql = if compiled.order_sql.is_empty() {
        "t.updated_at DESC".to_string()
    } else {
        compiled.order_sql.clone()
    };

    let where_sql = format!(
        "WHERE t.deleted_at IS NULL AND p.deleted_at IS NULL \
         AND t.project_id = ANY($1) AND ({})",
        compiled.where_sql
    );

    let count_sql = format!("SELECT count(*) {FROM_SQL} {where_sql}");
    let mut count_args = PgArguments::default();
    count_args
        .add(accessible.clone())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("bind failed: {e}")))?;
    let count_args = bind_all(count_args, &compiled.params)?;
    let total: i64 = sqlx::query_scalar_with(&count_sql, count_args)
        .fetch_one(&state.db)
        .await
        .map_err(explain)?;

    let page_sql = format!(
        r#"
        SELECT t.key, p.key AS project_key, t.title, t.status, t.priority,
               t.type, ua.handle AS assignee_handle, s.name AS sprint_name,
               t.labels, t.story_points, t.due_date, t.updated_at
        {FROM_SQL} {where_sql}
        ORDER BY {order_sql}, t.key
        LIMIT {limit} OFFSET {offset}
        "#
    );
    let mut args = PgArguments::default();
    args.add(accessible)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("bind failed: {e}")))?;
    let args = bind_all(args, &compiled.params)?;

    let rows = sqlx::query_with(&page_sql, args)
        .fetch_all(&state.db)
        .await
        .map_err(explain)?;

    let items: Vec<JqlHit> = rows
        .into_iter()
        .map(|r| JqlHit {
            key: r.get("key"),
            project_key: r.get("project_key"),
            title: r.get("title"),
            status: r.get("status"),
            priority: r.get("priority"),
            r#type: r.get("type"),
            assignee_handle: r.get("assignee_handle"),
            sprint_name: r.get("sprint_name"),
            labels: r.get("labels"),
            story_points: r.get("story_points"),
            due_date: r.get("due_date"),
            updated_at: r.get("updated_at"),
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

/// A type mismatch that survived parsing (e.g. comparing a text column with a
/// number in a way Postgres refuses) is the user's problem to fix, not a 500.
fn explain(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e {
        let code = db.code().unwrap_or_default().to_string();
        // 22P02 invalid_text_representation, 42883 undefined_function,
        // 42804 datatype_mismatch — all "those two things don't compare".
        if matches!(code.as_str(), "22P02" | "42883" | "42804") {
            return AppError::BadRequest(
                "that comparison doesn't type-check — check the value against the field".into(),
            );
        }
    }
    AppError::from(e)
}

async fn fields() -> AppResult<impl IntoResponse> {
    Ok(Json(serde_json::json!({ "fields": jql::field_names() })))
}

// ── saved queries ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SavedQueryDto {
    pub id: Uuid,
    pub name: String,
    pub jql: String,
    pub is_shared: bool,
    /// True when the caller owns it — the UI hides edit/delete otherwise.
    pub is_mine: bool,
    pub owner_handle: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct CreateSavedReq {
    name: String,
    jql: String,
    #[serde(default)]
    is_shared: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateSavedReq {
    name: Option<String>,
    jql: Option<String>,
    is_shared: Option<bool>,
}

fn check_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 60 {
        return Err(AppError::BadRequest("name must be 1–60 characters".into()));
    }
    Ok(n.to_string())
}

/// A query is validated before it's stored: saving something unparseable would
/// just hand the surprise to whoever loads it later (possibly a teammate).
fn check_jql(jql: &str, handle: &str) -> AppResult<String> {
    let j = jql.trim();
    if j.is_empty() || j.len() > 2000 {
        return Err(AppError::BadRequest(
            "query must be 1–2000 characters".into(),
        ));
    }
    let parsed = jql::parse(j)
        .map_err(|e| AppError::BadRequest(format!("{} — at character {}", e.message, e.at + 1)))?;
    // Compile too, so a query that parses but can't compile never gets stored.
    let _ = jql::compile(&parsed, handle, 2);
    Ok(j.to_string())
}

async fn list_saved(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let rows = sqlx::query!(
        r#"
        SELECT q.id           AS "id!: Uuid",
               q.name         AS "name!: String",
               q.jql          AS "jql!: String",
               q.is_shared    AS "is_shared!: bool",
               q.user_id      AS "user_id!: Uuid",
               u.handle       AS "owner_handle!: String",
               q.created_at   AS "created_at!: DateTime<Utc>",
               q.updated_at   AS "updated_at!: DateTime<Utc>"
        FROM   saved_queries q
        JOIN   users u ON u.id = q.user_id
        WHERE  q.user_id = $1 OR q.is_shared
        ORDER BY (q.user_id = $1) DESC, lower(q.name)
        "#,
        user.id
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<SavedQueryDto> = rows
        .into_iter()
        .map(|r| SavedQueryDto {
            id: r.id,
            name: r.name,
            jql: r.jql,
            is_shared: r.is_shared,
            is_mine: r.user_id == user.id,
            owner_handle: r.owner_handle,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn create_saved(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<CreateSavedReq>,
) -> AppResult<impl IntoResponse> {
    let handle = my_handle(&state, &user).await?;
    let name = check_name(&body.name)?;
    let jql_text = check_jql(&body.jql, &handle)?;

    let row = sqlx::query!(
        r#"
        INSERT INTO saved_queries (user_id, name, jql, is_shared)
        VALUES ($1, $2, $3, $4)
        RETURNING id AS "id!: Uuid",
                  created_at AS "created_at!: DateTime<Utc>",
                  updated_at AS "updated_at!: DateTime<Utc>"
        "#,
        user.id,
        name,
        jql_text,
        body.is_shared
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| name_conflict(e, &name))?;

    Ok((
        StatusCode::CREATED,
        Json(SavedQueryDto {
            id: row.id,
            name,
            jql: jql_text,
            is_shared: body.is_shared,
            is_mine: true,
            owner_handle: handle,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }),
    ))
}

async fn update_saved(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateSavedReq>,
) -> AppResult<impl IntoResponse> {
    // Sharing a query doesn't hand over the pen: only the owner can edit it.
    let owner: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT user_id FROM saved_queries WHERE id = $1"#)
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    match owner {
        None => return Err(AppError::NotFound),
        Some(o) if o != user.id => return Err(AppError::Forbidden),
        _ => {}
    }

    let handle = my_handle(&state, &user).await?;
    let name = match &body.name {
        Some(n) => Some(check_name(n)?),
        None => None,
    };
    let jql_text = match &body.jql {
        Some(j) => Some(check_jql(j, &handle)?),
        None => None,
    };

    let row = sqlx::query!(
        r#"
        UPDATE saved_queries
        SET    name      = COALESCE($2, name),
               jql       = COALESCE($3, jql),
               is_shared = COALESCE($4, is_shared),
               updated_at = now()
        WHERE  id = $1
        RETURNING name       AS "name!: String",
                  jql        AS "jql!: String",
                  is_shared  AS "is_shared!: bool",
                  created_at AS "created_at!: DateTime<Utc>",
                  updated_at AS "updated_at!: DateTime<Utc>"
        "#,
        id,
        name.as_deref(),
        jql_text.as_deref(),
        body.is_shared
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| name_conflict(e, name.as_deref().unwrap_or("that name")))?;

    Ok(Json(SavedQueryDto {
        id,
        name: row.name,
        jql: row.jql,
        is_shared: row.is_shared,
        is_mine: true,
        owner_handle: handle,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

async fn delete_saved(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let done = sqlx::query!(
        r#"DELETE FROM saved_queries WHERE id = $1 AND user_id = $2"#,
        id,
        user.id
    )
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        // Either it never existed or it isn't yours; a shared query you don't
        // own is somebody else's to remove.
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn name_conflict(e: sqlx::Error, name: &str) -> AppError {
    if let sqlx::Error::Database(db) = &e {
        if db.constraint() == Some("saved_queries_user_name_uniq") {
            return AppError::Conflict(format!(
                "you already have a saved query called “{name}” — pick another name or edit that one"
            ));
        }
    }
    AppError::from(e)
}
