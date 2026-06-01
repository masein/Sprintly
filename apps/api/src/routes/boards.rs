//! Boards + columns.
//!
//! Listing/creating boards is project-scoped:
//!   GET    /projects/:key/boards
//!   POST   /projects/:key/boards
//!
//! Once we hold a board id, board-level endpoints address it directly:
//!   GET    /boards/:id
//!   PATCH  /boards/:id
//!   POST   /boards/:id/columns
//!   PATCH  /boards/:id/columns/reorder
//!
//! And column-level:
//!   PATCH  /columns/:id
//!   DELETE /columns/:id
//!
//! Column sort_order is fractional. To insert between A (1024) and B (2048)
//! we use (A+B)/2 = 1536. The day precision drifts, we lazily renormalize
//! the whole column list — currently after every reorder we rebalance to
//! evenly-spaced powers of 1024 to keep things sane.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/boards", get(list_boards).post(create_board))
        .route("/boards/:id", get(get_board).patch(edit_board))
        .route("/boards/:id/columns", post(create_column))
        .route("/boards/:id/columns/reorder", post(reorder_columns))
        .route("/columns/:id", patch(edit_column).delete(delete_column))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BoardDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub r#type: String,
    pub is_default: bool,
    pub columns: Vec<ColumnDto>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ColumnDto {
    pub id: Uuid,
    pub board_id: Uuid,
    pub name: String,
    pub category: String,
    pub wip_limit: Option<i32>,
    pub sort_order: f64,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateBoardReq {
    #[validate(length(min = 1, max = 80))]
    pub name: String,
    /// "kanban" or "sprint".
    pub r#type: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditBoardReq {
    #[validate(length(min = 1, max = 80))]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateColumnReq {
    #[validate(length(min = 1, max = 60))]
    pub name: String,
    pub category: String,
    pub wip_limit: Option<i32>,
    /// Optional placement hint. If omitted, appended to the end.
    pub after_column_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditColumnReq {
    #[validate(length(min = 1, max = 60))]
    pub name: Option<String>,
    pub category: Option<String>,
    /// Set a new WIP limit. Send a positive integer to set, or omit to leave
    /// unchanged. (Clearing an existing limit gets its own endpoint in M9.)
    pub wip_limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderColumnsReq {
    /// Column IDs in their new left-to-right order.
    pub order: Vec<Uuid>,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn list_boards(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let boards = fetch_boards_with_columns(&state.db, ctx.id).await?;
    Ok(Json(serde_json::json!({ "items": boards })))
}

async fn create_board(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(req): Json<CreateBoardReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let board_type = match req.r#type.as_deref() {
        None | Some("kanban") => "kanban",
        Some("sprint") => "sprint",
        Some(_) => return Err(AppError::BadRequest("type must be kanban|sprint".into())),
    };
    let board_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO boards (id, project_id, name, type, is_default)
        VALUES ($1, $2, $3, $4, false)
        "#,
    )
    .bind(board_id)
    .bind(ctx.id)
    .bind(&req.name)
    .bind(board_type)
    .execute(&state.db)
    .await?;
    let dto = fetch_board(&state.db, board_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn get_board(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(board_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_id_of_board(&state.db, board_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let dto = fetch_board(&state.db, board_id).await?;
    Ok(Json(dto))
}

async fn edit_board(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(board_id): Path<Uuid>,
    Json(req): Json<EditBoardReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let project_id = project_id_of_board(&state.db, board_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE boards SET name = COALESCE($2, name) WHERE id = $1")
        .bind(board_id)
        .bind(req.name)
        .execute(&state.db)
        .await?;
    let dto = fetch_board(&state.db, board_id).await?;
    Ok(Json(dto))
}

async fn create_column(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(board_id): Path<Uuid>,
    Json(req): Json<CreateColumnReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if !matches!(
        req.category.as_str(),
        "todo" | "in_progress" | "review" | "done"
    ) {
        return Err(AppError::BadRequest("category invalid".into()));
    }
    let project_id = project_id_of_board(&state.db, board_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageColumns, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Compute sort_order: max + 1024, or fractional insert after a sibling.
    let sort_order = match req.after_column_id {
        None => next_after_max(&state.db, board_id).await?,
        Some(after) => fractional_insert_after(&state.db, board_id, after).await?,
    };

    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO board_columns (id, board_id, name, category, wip_limit, sort_order)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(board_id)
    .bind(&req.name)
    .bind(&req.category)
    .bind(req.wip_limit)
    .bind(sort_order)
    .execute(&state.db)
    .await?;

    let dto = fetch_column(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn edit_column(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(column_id): Path<Uuid>,
    Json(req): Json<EditColumnReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if let Some(cat) = req.category.as_deref() {
        if !matches!(cat, "todo" | "in_progress" | "review" | "done") {
            return Err(AppError::BadRequest("category invalid".into()));
        }
    }
    let project_id = project_id_of_column(&state.db, column_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageColumns, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    if let Some(n) = req.wip_limit {
        if n <= 0 {
            return Err(AppError::Validation("wip_limit must be > 0".into()));
        }
    }
    sqlx::query(
        r#"
        UPDATE board_columns SET
            name      = COALESCE($2, name),
            category  = COALESCE($3, category),
            wip_limit = COALESCE($4, wip_limit)
         WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(column_id)
    .bind(req.name)
    .bind(req.category)
    .bind(req.wip_limit)
    .execute(&state.db)
    .await?;
    let dto = fetch_column(&state.db, column_id).await?;
    Ok(Json(dto))
}

async fn delete_column(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(column_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_id_of_column(&state.db, column_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageColumns, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Refuse if the column still holds tasks. Force the user to move them
    // first — preserves history and avoids accidental data loss.
    let task_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM tasks
           WHERE column_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(column_id)
    .fetch_one(&state.db)
    .await?;
    if task_count > 0 {
        return Err(AppError::Conflict(
            "column still has tasks — move them first".into(),
        ));
    }

    sqlx::query("UPDATE board_columns SET deleted_at = now() WHERE id = $1")
        .bind(column_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reorder_columns(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(board_id): Path<Uuid>,
    Json(req): Json<ReorderColumnsReq>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_id_of_board(&state.db, board_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageColumns, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Validate: every id provided must belong to this board, and the set
    // sizes must match (no missing, no extras).
    let existing: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM board_columns WHERE board_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(board_id)
    .fetch_all(&state.db)
    .await?;
    if existing.len() != req.order.len() || !existing.iter().all(|e| req.order.contains(e)) {
        return Err(AppError::BadRequest(
            "reorder list must contain every active column exactly once".into(),
        ));
    }

    // Rebalance to clean spacing so we don't accumulate float drift.
    let mut tx = state.db.begin().await?;
    for (i, id) in req.order.iter().enumerate() {
        let so = ((i as f64) + 1.0) * 1024.0;
        sqlx::query("UPDATE board_columns SET sort_order = $2 WHERE id = $1")
            .bind(id)
            .bind(so)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── data fetchers ──────────────────────────────────────────────────────────

async fn project_id_of_board(db: &PgPool, board_id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"SELECT project_id FROM boards WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(board_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn project_id_of_column(db: &PgPool, column_id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT b.project_id
        FROM   board_columns c
        JOIN   boards b ON b.id = c.board_id
        WHERE  c.id = $1 AND c.deleted_at IS NULL AND b.deleted_at IS NULL
        "#,
    )
    .bind(column_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn next_after_max(db: &PgPool, board_id: Uuid) -> AppResult<f64> {
    let max: Option<f64> = sqlx::query_scalar(
        r#"SELECT MAX(sort_order) FROM board_columns
           WHERE board_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(board_id)
    .fetch_one(db)
    .await?;
    Ok(max.unwrap_or(0.0) + 1024.0)
}

async fn fractional_insert_after(db: &PgPool, board_id: Uuid, after: Uuid) -> AppResult<f64> {
    let after_so: f64 = sqlx::query_scalar(
        r#"SELECT sort_order FROM board_columns
           WHERE id = $1 AND board_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(after)
    .bind(board_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    let next_so: Option<f64> = sqlx::query_scalar(
        r#"SELECT MIN(sort_order) FROM board_columns
           WHERE board_id = $1 AND deleted_at IS NULL AND sort_order > $2"#,
    )
    .bind(board_id)
    .bind(after_so)
    .fetch_one(db)
    .await?;

    Ok(match next_so {
        Some(n) => (after_so + n) / 2.0,
        None => after_so + 1024.0,
    })
}

async fn fetch_boards_with_columns(db: &PgPool, project_id: Uuid) -> AppResult<Vec<BoardDto>> {
    let boards = sqlx::query!(
        r#"
        SELECT id          AS "id!: Uuid",
               project_id  AS "project_id!: Uuid",
               name        AS "name!: String",
               type        AS "type!: String",
               is_default  AS "is_default!: bool",
               created_at  AS "created_at!: DateTime<Utc>"
        FROM   boards
        WHERE  project_id = $1 AND deleted_at IS NULL
        ORDER  BY is_default DESC, created_at ASC
        "#,
        project_id
    )
    .fetch_all(db)
    .await?;

    let mut out = Vec::with_capacity(boards.len());
    for b in boards {
        let columns = fetch_columns_for(db, b.id).await?;
        out.push(BoardDto {
            id: b.id,
            project_id: b.project_id,
            name: b.name,
            r#type: b.r#type,
            is_default: b.is_default,
            columns,
            created_at: b.created_at,
        });
    }
    Ok(out)
}

async fn fetch_board(db: &PgPool, board_id: Uuid) -> AppResult<BoardDto> {
    let b = sqlx::query!(
        r#"
        SELECT id          AS "id!: Uuid",
               project_id  AS "project_id!: Uuid",
               name        AS "name!: String",
               type        AS "type!: String",
               is_default  AS "is_default!: bool",
               created_at  AS "created_at!: DateTime<Utc>"
        FROM   boards
        WHERE  id = $1 AND deleted_at IS NULL
        "#,
        board_id
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    let columns = fetch_columns_for(db, b.id).await?;
    Ok(BoardDto {
        id: b.id,
        project_id: b.project_id,
        name: b.name,
        r#type: b.r#type,
        is_default: b.is_default,
        columns,
        created_at: b.created_at,
    })
}

async fn fetch_columns_for(db: &PgPool, board_id: Uuid) -> AppResult<Vec<ColumnDto>> {
    let rows = sqlx::query!(
        r#"
        SELECT id         AS "id!: Uuid",
               board_id   AS "board_id!: Uuid",
               name       AS "name!: String",
               category   AS "category!: String",
               wip_limit,
               sort_order AS "sort_order!: f64"
        FROM   board_columns
        WHERE  board_id = $1 AND deleted_at IS NULL
        ORDER  BY sort_order ASC
        "#,
        board_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ColumnDto {
            id: r.id,
            board_id: r.board_id,
            name: r.name,
            category: r.category,
            wip_limit: r.wip_limit,
            sort_order: r.sort_order,
        })
        .collect())
}

async fn fetch_column(db: &PgPool, column_id: Uuid) -> AppResult<ColumnDto> {
    let r = sqlx::query!(
        r#"
        SELECT id         AS "id!: Uuid",
               board_id   AS "board_id!: Uuid",
               name       AS "name!: String",
               category   AS "category!: String",
               wip_limit,
               sort_order AS "sort_order!: f64"
        FROM   board_columns
        WHERE  id = $1 AND deleted_at IS NULL
        "#,
        column_id
    )
    .fetch_one(db)
    .await?;
    Ok(ColumnDto {
        id: r.id,
        board_id: r.board_id,
        name: r.name,
        category: r.category,
        wip_limit: r.wip_limit,
        sort_order: r.sort_order,
    })
}
