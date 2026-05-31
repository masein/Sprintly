//! Retro endpoints.
//!
//!   GET    /sprints/:id/retro                     — full retro snapshot
//!   POST   /retros/:id/notes                      — create note (anonymous?)
//!   PATCH  /retro-notes/:id                       — edit own note (or admin)
//!   DELETE /retro-notes/:id                       — soft delete (own / admin)
//!   POST   /retro-notes/:id/vote                  — vote (idempotent)
//!   DELETE /retro-notes/:id/vote                  — unvote
//!   POST   /retro-notes/:id/promote               — action_item → new task
//!   POST   /retros/:id/close                      — generate markdown summary
//!                                                   and store on sprint
//!
//! Anonymous notes: `anonymous=true` stores author_id = NULL (defense in
//! depth — even a careless future query can't leak the writer).

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action, Role as GlobalRole},
        projects as project_ctx,
        sprints as sprint_domain,
        tasks as task_domain,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sprints/:id/retro", get(get_retro))
        .route("/retros/:id/notes", post(create_note))
        .route(
            "/retro-notes/:id",
            axum::routing::patch(edit_note).delete(delete_note),
        )
        .route(
            "/retro-notes/:id/vote",
            post(vote).delete(unvote),
        )
        .route("/retro-notes/:id/promote", post(promote))
        .route("/retros/:id/close", post(close_retro))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RetroDto {
    pub id: Uuid,
    pub sprint_id: Uuid,
    pub state: String,
    pub notes: HashMap<String, Vec<NoteDto>>,
}

#[derive(Debug, Serialize)]
pub struct NoteDto {
    pub id: Uuid,
    pub column_kind: String,
    pub body: String,
    pub anonymous: bool,
    /// Hidden when anonymous.
    pub author_handle: Option<String>,
    pub vote_count: i64,
    pub you_voted: bool,
    pub promoted_task_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateNoteReq {
    pub column_kind: String,
    #[validate(length(min = 1, max = 4000))]
    pub body: String,
    pub anonymous: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditNoteReq {
    #[validate(length(min = 1, max = 4000))]
    pub body: String,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn get_retro(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sprint_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, sprint_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let retro = sqlx::query!(
        r#"
        SELECT id    AS "id!: Uuid",
               state AS "state!: String"
        FROM   sprint_retros
        WHERE  sprint_id = $1
        "#,
        sprint_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let rows = sqlx::query!(
        r#"
        SELECT n.id              AS "id!: Uuid",
               n.column_kind     AS "column_kind!: String",
               n.body            AS "body!: String",
               n.anonymous       AS "anonymous!: bool",
               u.handle          AS "author_handle?: String",
               n.created_at      AS "created_at!: DateTime<Utc>",
               n.promoted_task_id,
               (SELECT key FROM tasks t WHERE t.id = n.promoted_task_id)
                                 AS "promoted_task_key?: String",
               (SELECT COUNT(*) FROM retro_votes v WHERE v.retro_note_id = n.id)
                                 AS "vote_count!: i64",
               EXISTS(SELECT 1 FROM retro_votes v WHERE v.retro_note_id = n.id AND v.user_id = $2)
                                 AS "you_voted!: bool"
        FROM   retro_notes n
        LEFT JOIN users u ON u.id = n.author_id
        WHERE  n.retro_id = $1 AND n.deleted_at IS NULL
        ORDER  BY n.column_kind, n.sort_order ASC, n.created_at ASC
        "#,
        retro.id,
        user.id
    )
    .fetch_all(&state.db)
    .await?;

    let mut buckets: HashMap<String, Vec<NoteDto>> = HashMap::new();
    for k in ["went_well", "went_poorly", "action_item", "kudos"] {
        buckets.insert(k.into(), Vec::new());
    }
    for r in rows {
        let bucket = buckets
            .entry(r.column_kind.clone())
            .or_insert_with(Vec::new);
        let anon = r.anonymous;
        bucket.push(NoteDto {
            id: r.id,
            column_kind: r.column_kind,
            body: r.body,
            anonymous: anon,
            author_handle: if anon { None } else { r.author_handle },
            vote_count: r.vote_count,
            you_voted: r.you_voted,
            promoted_task_key: r.promoted_task_key,
            created_at: r.created_at,
        });
    }

    Ok(Json(RetroDto {
        id: retro.id,
        sprint_id,
        state: retro.state,
        notes: buckets,
    }))
}

async fn create_note(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(retro_id): Path<Uuid>,
    Json(req): Json<CreateNoteReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if !matches!(
        req.column_kind.as_str(),
        "went_well" | "went_poorly" | "action_item" | "kudos"
    ) {
        return Err(AppError::BadRequest("column_kind invalid".into()));
    }
    // Resolve project for access check + ensure retro is open.
    let (sprint_id, retro_state) = sqlx::query!(
        r#"
        SELECT sprint_id AS "sprint_id!: Uuid",
               state     AS "state!: String"
        FROM   sprint_retros WHERE id = $1
        "#,
        retro_id
    )
    .fetch_optional(&state.db)
    .await?
    .map(|r| (r.sprint_id, r.state))
    .ok_or(AppError::NotFound)?;
    if retro_state != "open" {
        return Err(AppError::Conflict("retro is closed".into()));
    }
    let project_id = project_of_sprint(&state.db, sprint_id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let anonymous = req.anonymous.unwrap_or(false);
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO retro_notes (id, retro_id, author_id, column_kind, body, anonymous)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(retro_id)
    .bind(if anonymous { None } else { Some(user.id) })
    .bind(&req.column_kind)
    .bind(&req.body)
    .bind(anonymous)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::CREATED)
}

async fn edit_note(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<EditNoteReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let row = sqlx::query!(
        r#"SELECT author_id, anonymous AS "anonymous!: bool"
           FROM retro_notes WHERE id = $1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    // Anonymous notes have no owner; only admins can edit them.
    let is_admin = user.role == GlobalRole::Admin;
    let is_author = !row.anonymous && row.author_id == Some(user.id);
    if !is_author && !is_admin {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE retro_notes SET body = $1 WHERE id = $2")
        .bind(&req.body)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_note(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let row = sqlx::query!(
        r#"SELECT author_id, anonymous AS "anonymous!: bool"
           FROM retro_notes WHERE id = $1 AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let is_admin = user.role == GlobalRole::Admin;
    let is_author = !row.anonymous && row.author_id == Some(user.id);
    if !is_author && !is_admin {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE retro_notes SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn vote(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    sqlx::query(
        r#"INSERT INTO retro_votes (retro_note_id, user_id) VALUES ($1, $2)
           ON CONFLICT DO NOTHING"#,
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unvote(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    sqlx::query("DELETE FROM retro_votes WHERE retro_note_id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn promote(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    // Look up note + sprint + project. Only action_item rows are promotable.
    let row = sqlx::query!(
        r#"
        SELECT n.column_kind     AS "column_kind!: String",
               n.body            AS "body!: String",
               n.promoted_task_id,
               s.project_id      AS "project_id!: Uuid"
        FROM   retro_notes n
        JOIN   sprint_retros r ON r.id = n.retro_id
        JOIN   sprints s ON s.id = r.sprint_id
        WHERE  n.id = $1 AND n.deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if row.column_kind != "action_item" {
        return Err(AppError::BadRequest(
            "only action items can be promoted to tasks".into(),
        ));
    }
    if row.promoted_task_id.is_some() {
        return Err(AppError::Conflict("already promoted".into()));
    }
    let ctx = project_ctx::load_by_id(&state.db, row.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Find the project's default board + first column.
    let placement = sqlx::query!(
        r#"
        SELECT b.id  AS "board_id!: Uuid",
               bc.id AS "column_id!: Uuid"
        FROM   boards b
        JOIN   board_columns bc ON bc.board_id = b.id AND bc.deleted_at IS NULL
        WHERE  b.project_id = $1 AND b.is_default = true AND b.deleted_at IS NULL
        ORDER  BY bc.sort_order ASC
        LIMIT  1
        "#,
        row.project_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Conflict("project has no default board".into()))?;

    // Compute task title — single line, truncated; full body lands in description.
    let title = first_line(&row.body, 200);

    let mut tx = state.db.begin().await?;
    let (task_key, _seq) = task_domain::next_key(&mut tx, row.project_id).await?;
    let task_id = Uuid::now_v7();
    let category: String =
        sqlx::query_scalar(r#"SELECT category FROM board_columns WHERE id = $1"#)
            .bind(placement.column_id)
            .fetch_one(&mut *tx)
            .await?;
    let max_order: Option<f64> = sqlx::query_scalar(
        r#"SELECT MAX(order_in_column) FROM tasks
           WHERE column_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(placement.column_id)
    .fetch_one(&mut *tx)
    .await?;
    let new_order = max_order.unwrap_or(0.0) + 1024.0;

    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, project_id, board_id, column_id, key, title, description,
            type, priority, status, reporter_id, order_in_column
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            'chore', 'p2', $8, $9, $10
        )
        "#,
    )
    .bind(task_id)
    .bind(row.project_id)
    .bind(placement.board_id)
    .bind(placement.column_id)
    .bind(&task_key)
    .bind(&title)
    .bind(&row.body)
    .bind(&category)
    .bind(user.id)
    .bind(new_order)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE retro_notes SET promoted_task_id = $1 WHERE id = $2")
        .bind(task_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    task_domain::log_activity(
        &mut tx,
        task_id,
        Some(user.id),
        "created",
        &serde_json::json!({ "from_retro_note": id }),
    )
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "task_key": task_key })),
    ))
}

async fn close_retro(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(retro_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let row = sqlx::query!(
        r#"
        SELECT r.state           AS "state!: String",
               r.sprint_id       AS "sprint_id!: Uuid",
               s.project_id      AS "project_id!: Uuid",
               s.name            AS "name!: String",
               s.goal            AS "goal!: String",
               s.starts_at       AS "starts_at!: DateTime<Utc>",
               s.ends_at         AS "ends_at!: DateTime<Utc>",
               s.velocity_points
        FROM   sprint_retros r
        JOIN   sprints s ON s.id = r.sprint_id
        WHERE  r.id = $1
        "#,
        retro_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if row.state == "closed" {
        return Err(AppError::Conflict("retro already closed".into()));
    }
    let ctx = project_ctx::load_by_id(&state.db, row.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Gather notes per column, ordered by votes-desc then created_at-asc.
    let notes = sqlx::query!(
        r#"
        SELECT n.column_kind     AS "column_kind!: String",
               n.body            AS "body!: String",
               (SELECT COUNT(*) FROM retro_votes v WHERE v.retro_note_id = n.id)
                                 AS "vote_count!: i64"
        FROM   retro_notes n
        WHERE  n.retro_id = $1 AND n.deleted_at IS NULL
        ORDER  BY n.column_kind,
                  (SELECT COUNT(*) FROM retro_votes v WHERE v.retro_note_id = n.id) DESC,
                  n.created_at ASC
        "#,
        retro_id
    )
    .fetch_all(&state.db)
    .await?;

    let mut went_well = Vec::new();
    let mut went_poorly = Vec::new();
    let mut action_items = Vec::new();
    let mut kudos = Vec::new();
    for n in &notes {
        let bullet = if n.vote_count > 0 {
            format!("{} (+{} votes)", n.body, n.vote_count)
        } else {
            n.body.clone()
        };
        let bucket = match n.column_kind.as_str() {
            "went_well" => &mut went_well,
            "went_poorly" => &mut went_poorly,
            "action_item" => &mut action_items,
            "kudos" => &mut kudos,
            _ => continue,
        };
        bucket.push(bullet);
    }

    let completed_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM tasks
        WHERE  sprint_id = $1 AND status = 'done' AND deleted_at IS NULL
        "#,
    )
    .bind(row.sprint_id)
    .fetch_one(&state.db)
    .await?;
    let carried_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM tasks
        WHERE  sprint_id = $1 AND status <> 'done' AND deleted_at IS NULL
        "#,
    )
    .bind(row.sprint_id)
    .fetch_one(&state.db)
    .await?;

    let summary = sprint_domain::retro_summary_markdown(&sprint_domain::RetroSummaryInput {
        sprint_name: &row.name,
        sprint_goal: &row.goal,
        starts: row.starts_at.date_naive(),
        ends: row.ends_at.date_naive(),
        velocity_points: row.velocity_points.map(|v| v as i64),
        completed_count,
        carried_count,
        went_well: went_well.iter().map(String::as_str).collect(),
        went_poorly: went_poorly.iter().map(String::as_str).collect(),
        action_items: action_items.iter().map(String::as_str).collect(),
        kudos: kudos.iter().map(String::as_str).collect(),
    });

    let mut tx = state.db.begin().await?;
    sqlx::query(r#"UPDATE sprint_retros SET state = 'closed', closed_at = now() WHERE id = $1"#)
        .bind(retro_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"UPDATE sprints SET summary_md = $2 WHERE id = $1"#)
        .bind(row.sprint_id)
        .bind(&summary)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "summary_md": summary })))
}

// ─── helpers ────────────────────────────────────────────────────────────────

async fn project_of_sprint(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"SELECT project_id FROM sprints WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        // Truncate by chars to keep it UTF-8 safe.
        let mut out: String = line.chars().take(max).collect();
        out.push('…');
        out
    }
}

// Keep `Datelike` referenced for completeness — date_naive() pairs with it
// downstream; some readers prefer it imported alongside the other chrono
// types they use.
#[allow(dead_code)]
fn _datelike_marker(d: NaiveDate) -> u32 { d.year() as u32 }
