//! Everything the task detail page needs:
//!
//!   Comments       — GET/POST /tasks/:key/comments, PATCH/DELETE /comments/:id
//!   Reactions      — POST/DELETE /reactions  (on task XOR comment)
//!   Activity feed  — GET /tasks/:key/activity
//!   Watchers       — GET/POST/DELETE /tasks/:key/watchers[/:user_id]
//!   Links          — POST /tasks/:key/links, DELETE /tasks/:key/links
//!   Attachments    — POST /tasks/:key/attachments (presign PUT),
//!                    POST /attachments/:id/complete,
//!                    GET  /tasks/:key/attachments (presigned GETs),
//!                    DELETE /attachments/:id

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
        permissions::Role as GlobalRole,
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::{events::Event, s3::Presigner, AppState},
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/tasks/:task_key/comments",
            get(list_comments).post(create_comment),
        )
        .route(
            "/comments/:id",
            axum::routing::patch(edit_comment).delete(delete_comment),
        )
        .route("/reactions", post(add_reaction))
        .route("/reactions/:id", delete(remove_reaction))
        .route("/tasks/:task_key/activity", get(list_activity))
        .route(
            "/tasks/:task_key/watchers",
            get(list_watchers).post(add_watcher),
        )
        .route("/tasks/:task_key/watchers/:user_id", delete(remove_watcher))
        .route(
            "/tasks/:task_key/links",
            get(list_links).post(add_link).delete(remove_link),
        )
        .route("/tasks/:task_key/subtasks", get(list_subtasks))
        .route(
            "/tasks/:task_key/attachments",
            post(create_attachment).get(list_attachments),
        )
        .route("/attachments/:id", delete(delete_attachment))
        .route("/attachments/:id/complete", post(complete_attachment))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CommentDto {
    pub id: Uuid,
    pub task_id: Uuid,
    pub author_id: Option<Uuid>,
    pub author_handle: Option<String>,
    pub parent_comment_id: Option<Uuid>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub reactions: Vec<ReactionGroupDto>,
}

#[derive(Debug, Serialize)]
pub struct ReactionGroupDto {
    pub emoji: String,
    pub count: i64,
    pub user_reacted: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateCommentReq {
    #[validate(length(min = 1, max = 10_000))]
    pub body: String,
    pub parent_comment_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditCommentReq {
    #[validate(length(min = 1, max = 10_000))]
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct ReactionTarget {
    pub task_key: Option<String>,
    pub comment_id: Option<Uuid>,
    pub emoji: String,
}

#[derive(Debug, Serialize)]
pub struct ActivityDto {
    pub id: Uuid,
    pub task_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub actor_handle: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct WatcherDto {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AddWatcherReq {
    pub user_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct LinkReq {
    pub to_task_key: String,
    pub kind: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateAttachmentReq {
    #[validate(length(min = 1, max = 255))]
    pub filename: String,
    #[validate(length(min = 1, max = 200))]
    pub mime_type: String,
}

#[derive(Debug, Serialize)]
pub struct AttachmentInitDto {
    pub id: Uuid,
    pub upload_url: String,
    pub storage_key: String,
    pub expires_in: u32,
}

#[derive(Debug, Deserialize)]
pub struct CompleteAttachmentReq {
    pub size_bytes: i64,
    pub checksum: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AttachmentDto {
    pub id: Uuid,
    pub task_id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub download_url: Option<String>,
    pub uploader_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ─── handlers: comments ─────────────────────────────────────────────────────

async fn list_comments(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let rows = sqlx::query!(
        r#"
        SELECT c.id            AS "id!: Uuid",
               c.task_id       AS "task_id!: Uuid",
               c.author_id,
               u.handle        AS "author_handle?: String",
               c.parent_comment_id,
               c.body          AS "body!: String",
               c.created_at    AS "created_at!: DateTime<Utc>",
               c.edited_at
        FROM   task_comments c
        LEFT JOIN users u ON u.id = c.author_id
        WHERE  c.task_id = $1 AND c.deleted_at IS NULL
        ORDER BY c.created_at ASC
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;

    // Fetch reactions for all comments in one shot to avoid N+1.
    let comment_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let reactions = if comment_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query!(
            r#"
            SELECT comment_id  AS "comment_id!: Uuid",
                   emoji       AS "emoji!: String",
                   COUNT(*)    AS "count!: i64",
                   bool_or(user_id = $2) AS "user_reacted!: bool"
            FROM   task_reactions
            WHERE  comment_id = ANY($1)
            GROUP  BY comment_id, emoji
            ORDER  BY emoji
            "#,
            &comment_ids,
            user.id
        )
        .fetch_all(&state.db)
        .await?
    };

    let items: Vec<CommentDto> = rows
        .into_iter()
        .map(|r| CommentDto {
            id: r.id,
            task_id: r.task_id,
            author_id: r.author_id,
            author_handle: r.author_handle,
            parent_comment_id: r.parent_comment_id,
            body: r.body,
            created_at: r.created_at,
            edited_at: r.edited_at,
            reactions: reactions
                .iter()
                .filter(|x| x.comment_id == r.id)
                .map(|x| ReactionGroupDto {
                    emoji: x.emoji.clone(),
                    count: x.count,
                    user_reacted: x.user_reacted,
                })
                .collect(),
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn create_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<CreateCommentReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        // Members & contributors can comment; viewers can't.
        return Err(AppError::Forbidden);
    }

    let id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    let insert = sqlx::query(
        r#"
        INSERT INTO task_comments (id, task_id, author_id, parent_comment_id, body)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(task.id)
    .bind(user.id)
    .bind(req.parent_comment_id)
    .bind(&req.body)
    .execute(&mut *tx)
    .await;

    if let Err(sqlx::Error::Database(db)) = &insert {
        // Threading trigger raises a generic exception.
        if let Some(msg) = db.message().split('\n').next() {
            if msg.contains("one level deep") {
                return Err(AppError::BadRequest(
                    "replies can be one level deep only".into(),
                ));
            }
        }
    }
    insert?;

    crate::domain::tasks::log_activity(
        &mut tx,
        task.id,
        Some(user.id),
        "commented",
        &serde_json::json!({
            "comment_id": id,
        }),
    )
    .await?;
    tx.commit().await?;

    crate::infra::events::publish(
        &state.redis,
        &Event::CommentCreated {
            project_id: task.project_id,
            task_id: task.id,
            comment_id: id,
        },
    )
    .await;

    let dto = fetch_one_comment(&state.db, id, user.id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn edit_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<EditCommentReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let c = sqlx::query!(
        r#"
        SELECT task_id   AS "task_id!: Uuid",
               author_id
        FROM   task_comments WHERE id = $1 AND deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if user.role != GlobalRole::Admin && c.author_id != Some(user.id) {
        return Err(AppError::Forbidden);
    }

    sqlx::query(r#"UPDATE task_comments SET body = $1, edited_at = now() WHERE id = $2"#)
        .bind(&req.body)
        .bind(id)
        .execute(&state.db)
        .await?;

    let dto = fetch_one_comment(&state.db, id, user.id).await?;
    Ok(Json(dto))
}

async fn delete_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let c = sqlx::query!(
        r#"SELECT author_id FROM task_comments WHERE id = $1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if user.role != GlobalRole::Admin && c.author_id != Some(user.id) {
        return Err(AppError::Forbidden);
    }

    sqlx::query("UPDATE task_comments SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── handlers: reactions ────────────────────────────────────────────────────

async fn add_reaction(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<ReactionTarget>,
) -> AppResult<impl IntoResponse> {
    // Must specify exactly one target.
    let (task_id, comment_id) = match (req.task_key.as_deref(), req.comment_id) {
        (Some(k), None) => (Some(resolve_task(&state.db, k).await?.id), None),
        (None, Some(cid)) => (None, Some(cid)),
        _ => {
            return Err(AppError::BadRequest(
                "target task_key XOR comment_id".into(),
            ))
        }
    };

    if !is_emoji_ok(&req.emoji) {
        return Err(AppError::BadRequest("emoji invalid".into()));
    }

    let insert = sqlx::query(
        r#"
        INSERT INTO task_reactions (id, task_id, comment_id, user_id, emoji)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(task_id)
    .bind(comment_id)
    .bind(user.id)
    .bind(&req.emoji)
    .execute(&state.db)
    .await?;

    if insert.rows_affected() == 0 {
        return Ok(StatusCode::OK); // already reacted; idempotent
    }
    Ok(StatusCode::CREATED)
}

async fn remove_reaction(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let n = sqlx::query("DELETE FROM task_reactions WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── handlers: activity ─────────────────────────────────────────────────────

async fn list_activity(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT a.id          AS "id!: Uuid",
               a.task_id     AS "task_id!: Uuid",
               a.actor_id,
               u.handle      AS "actor_handle?: String",
               a.kind        AS "kind!: String",
               a.payload     AS "payload!: serde_json::Value",
               a.created_at  AS "created_at!: DateTime<Utc>"
        FROM   task_activity a
        LEFT JOIN users u ON u.id = a.actor_id
        WHERE  a.task_id = $1
        ORDER  BY a.created_at DESC
        LIMIT  200
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<ActivityDto> = rows
        .into_iter()
        .map(|r| ActivityDto {
            id: r.id,
            task_id: r.task_id,
            actor_id: r.actor_id,
            actor_handle: r.actor_handle,
            kind: r.kind,
            payload: r.payload,
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

// ─── handlers: watchers ─────────────────────────────────────────────────────

async fn list_watchers(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT w.user_id      AS "user_id!: Uuid",
               u.handle       AS "handle!: String",
               u.display_name AS "display_name!: String",
               w.added_at     AS "added_at!: DateTime<Utc>"
        FROM   task_watchers w
        JOIN   users u ON u.id = w.user_id
        WHERE  w.task_id = $1 AND u.deleted_at IS NULL
        ORDER  BY u.handle
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<WatcherDto> = rows
        .into_iter()
        .map(|r| WatcherDto {
            user_id: r.user_id,
            handle: r.handle,
            display_name: r.display_name,
            added_at: r.added_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn add_watcher(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<AddWatcherReq>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    // Self-add is always allowed (if you can view, you can watch yourself).
    let actor_can_view = can(&user.as_actor(), Action::ViewBoard, ctx.as_resource());
    let adding_self = req.user_id == user.id;
    if !actor_can_view
        || (!adding_self && !can(&user.as_actor(), Action::EditProject, ctx.as_resource()))
    {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        INSERT INTO task_watchers (task_id, user_id) VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(task.id)
    .bind(req.user_id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_watcher(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((task_key, target)): Path<(String, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    let removing_self = target == user.id;
    if !removing_self && !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query("DELETE FROM task_watchers WHERE task_id = $1 AND user_id = $2")
        .bind(task.id)
        .bind(target)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── handlers: links ────────────────────────────────────────────────────────

async fn add_link(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<LinkReq>,
) -> AppResult<impl IntoResponse> {
    if !matches!(
        req.kind.as_str(),
        "blocks" | "relates_to" | "duplicates" | "parent_of"
    ) {
        return Err(AppError::BadRequest("kind invalid".into()));
    }
    let from = resolve_task(&state.db, &task_key).await?;
    let to = resolve_task(&state.db, &req.to_task_key).await?;
    if from.id == to.id {
        return Err(AppError::BadRequest("cannot link a task to itself".into()));
    }
    let ctx = project_ctx::load_by_id(&state.db, from.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        INSERT INTO task_links (from_task_id, to_task_id, kind) VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(from.id)
    .bind(to.id)
    .bind(&req.kind)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_link(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<LinkReq>,
) -> AppResult<impl IntoResponse> {
    let from = resolve_task(&state.db, &task_key).await?;
    let to = resolve_task(&state.db, &req.to_task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, from.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"DELETE FROM task_links
           WHERE from_task_id = $1 AND to_task_id = $2 AND kind = $3"#,
    )
    .bind(from.id)
    .bind(to.id)
    .bind(&req.kind)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── handlers: links list + subtasks list ──────────────────────────────────

#[derive(Debug, Serialize)]
pub struct LinkDto {
    pub kind: String,
    pub direction: String, // "outgoing" | "incoming"
    pub other_task_key: String,
    pub other_task_title: String,
    pub other_status: String,
}

async fn list_links(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT l.kind         AS "kind!: String",
               'outgoing'     AS "direction!: String",
               t2.key         AS "other_key!: String",
               t2.title       AS "other_title!: String",
               t2.status      AS "other_status!: String"
        FROM   task_links l
        JOIN   tasks t2 ON t2.id = l.to_task_id
        WHERE  l.from_task_id = $1 AND t2.deleted_at IS NULL
        UNION ALL
        SELECT l.kind,
               'incoming',
               t1.key,
               t1.title,
               t1.status
        FROM   task_links l
        JOIN   tasks t1 ON t1.id = l.from_task_id
        WHERE  l.to_task_id = $1 AND t1.deleted_at IS NULL
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<LinkDto> = rows
        .into_iter()
        .map(|r| LinkDto {
            kind: r.kind,
            direction: r.direction,
            other_task_key: r.other_key,
            other_task_title: r.other_title,
            other_status: r.other_status,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, Serialize)]
pub struct SubtaskDto {
    pub key: String,
    pub title: String,
    pub status: String,
    pub assignee_id: Option<Uuid>,
}

async fn list_subtasks(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT key    AS "key!: String",
               title  AS "title!: String",
               status AS "status!: String",
               assignee_id
        FROM   tasks
        WHERE  parent_task_id = $1 AND deleted_at IS NULL
        ORDER BY created_at ASC
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<SubtaskDto> = rows
        .into_iter()
        .map(|r| SubtaskDto {
            key: r.key,
            title: r.title,
            status: r.status,
            assignee_id: r.assignee_id,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

// ─── handlers: attachments ──────────────────────────────────────────────────

async fn create_attachment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<CreateAttachmentReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let id = Uuid::now_v7();
    let storage_key = format!("tasks/{}/{}", task.id, id);
    sqlx::query(
        r#"
        INSERT INTO task_attachments (
            id, task_id, uploader_id, filename, mime_type, storage_key, status
        ) VALUES ($1, $2, $3, $4, $5, $6, 'pending')
        "#,
    )
    .bind(id)
    .bind(task.id)
    .bind(user.id)
    .bind(&req.filename)
    .bind(&req.mime_type)
    .bind(&storage_key)
    .execute(&state.db)
    .await?;

    let signer = Presigner::new(&state.cfg.minio);
    let upload_url = signer.put(&storage_key, &req.mime_type, 600);

    Ok((
        StatusCode::CREATED,
        Json(AttachmentInitDto {
            id,
            upload_url,
            storage_key,
            expires_in: 600,
        }),
    ))
}

async fn complete_attachment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CompleteAttachmentReq>,
) -> AppResult<impl IntoResponse> {
    let row = sqlx::query!(
        r#"
        SELECT task_id     AS "task_id!: Uuid",
               uploader_id,
               status      AS "status!: String"
        FROM   task_attachments WHERE id = $1 AND deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    // Only the uploader (or an admin) can complete.
    if user.role != GlobalRole::Admin && row.uploader_id != Some(user.id) {
        return Err(AppError::Forbidden);
    }
    if row.status != "pending" {
        return Err(AppError::Conflict("attachment already finalized".into()));
    }
    sqlx::query(
        r#"
        UPDATE task_attachments
           SET status = 'ready', size_bytes = $2, checksum = $3
         WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(req.size_bytes)
    .bind(req.checksum.as_deref())
    .execute(&state.db)
    .await?;

    // Activity stamp.
    let mut tx = state.db.begin().await?;
    crate::domain::tasks::log_activity(
        &mut tx,
        row.task_id,
        Some(user.id),
        "attached",
        &serde_json::json!({ "attachment_id": id }),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_attachments(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let task = resolve_task(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_id(&state.db, task.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let rows = sqlx::query!(
        r#"
        SELECT id          AS "id!: Uuid",
               task_id     AS "task_id!: Uuid",
               filename    AS "filename!: String",
               mime_type   AS "mime_type!: String",
               size_bytes,
               storage_key AS "storage_key!: String",
               status      AS "status!: String",
               uploader_id,
               created_at  AS "created_at!: DateTime<Utc>"
        FROM   task_attachments
        WHERE  task_id = $1 AND deleted_at IS NULL
        ORDER  BY created_at DESC
        "#,
        task.id
    )
    .fetch_all(&state.db)
    .await?;

    let signer = Presigner::new(&state.cfg.minio);
    let items: Vec<AttachmentDto> = rows
        .into_iter()
        .map(|r| AttachmentDto {
            id: r.id,
            task_id: r.task_id,
            filename: r.filename.clone(),
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
            status: r.status.clone(),
            download_url: if r.status == "ready" {
                Some(signer.get(&r.storage_key, Some(&r.filename), 600))
            } else {
                None
            },
            uploader_id: r.uploader_id,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items })))
}

async fn delete_attachment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let row = sqlx::query!(
        r#"
        SELECT a.task_id     AS "task_id!: Uuid",
               a.uploader_id,
               b.project_id  AS "project_id!: Uuid"
        FROM   task_attachments a
        JOIN   tasks t ON t.id = a.task_id
        JOIN   boards b ON b.id = t.board_id
        WHERE  a.id = $1 AND a.deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let ctx = project_ctx::load_by_id(&state.db, row.project_id, user.id).await?;
    let is_uploader = row.uploader_id == Some(user.id);
    let can_admin_delete = can(&user.as_actor(), Action::EditProject, ctx.as_resource());
    if !is_uploader && !can_admin_delete {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE task_attachments SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    // TODO(sprintly): a background job removes the underlying MinIO object.
    Ok(StatusCode::NO_CONTENT)
}

// ─── shared helpers ─────────────────────────────────────────────────────────

struct TaskRef {
    id: Uuid,
    project_id: Uuid,
}

async fn resolve_task(db: &PgPool, task_key: &str) -> AppResult<TaskRef> {
    let row = sqlx::query!(
        r#"
        SELECT id         AS "id!: Uuid",
               project_id AS "project_id!: Uuid"
        FROM   tasks
        WHERE  key = $1 AND deleted_at IS NULL
        "#,
        task_key
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(TaskRef {
        id: row.id,
        project_id: row.project_id,
    })
}

async fn fetch_one_comment(db: &PgPool, id: Uuid, viewer: Uuid) -> AppResult<CommentDto> {
    let r = sqlx::query!(
        r#"
        SELECT c.id            AS "id!: Uuid",
               c.task_id       AS "task_id!: Uuid",
               c.author_id,
               u.handle        AS "author_handle?: String",
               c.parent_comment_id,
               c.body          AS "body!: String",
               c.created_at    AS "created_at!: DateTime<Utc>",
               c.edited_at
        FROM   task_comments c
        LEFT JOIN users u ON u.id = c.author_id
        WHERE  c.id = $1 AND c.deleted_at IS NULL
        "#,
        id
    )
    .fetch_one(db)
    .await?;
    let reactions = sqlx::query!(
        r#"
        SELECT emoji      AS "emoji!: String",
               COUNT(*)   AS "count!: i64",
               bool_or(user_id = $2) AS "user_reacted!: bool"
        FROM   task_reactions
        WHERE  comment_id = $1
        GROUP  BY emoji
        ORDER  BY emoji
        "#,
        id,
        viewer
    )
    .fetch_all(db)
    .await?;
    Ok(CommentDto {
        id: r.id,
        task_id: r.task_id,
        author_id: r.author_id,
        author_handle: r.author_handle,
        parent_comment_id: r.parent_comment_id,
        body: r.body,
        created_at: r.created_at,
        edited_at: r.edited_at,
        reactions: reactions
            .into_iter()
            .map(|x| ReactionGroupDto {
                emoji: x.emoji,
                count: x.count,
                user_reacted: x.user_reacted,
            })
            .collect(),
    })
}

/// Minimal allowlist — we accept basic unicode emoji and short shortcode
/// strings (e.g. `:+1:`). Reject anything with line breaks or > 16 bytes.
fn is_emoji_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16 && !s.contains(['\n', '\r', '\t'])
}
