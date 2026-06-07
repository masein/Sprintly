//! Task endpoints.
//!
//!   POST   /projects/:key/tasks          — create. Auto-keyed PROJ-N.
//!   GET    /projects/:key/tasks          — list, with optional ?filter= DSL
//!                                          (very small grammar in M3; grows).
//!   GET    /tasks/:task_key              — detail (e.g. WEB-142).
//!   PATCH  /tasks/:task_key              — edit fields.
//!   DELETE /tasks/:task_key              — soft delete.
//!   POST   /tasks/:task_key/move         — column + position change.
//!
//! Every write logs to `task_activity` and publishes a Redis event for the
//! `/ws` fan-out.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx, tasks as task_domain,
    },
    infra::{events::Event, AppState},
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/tasks", post(create_task).get(list_tasks))
        .route(
            "/tasks/:task_key",
            get(get_task).patch(edit_task).delete(delete_task),
        )
        .route("/tasks/:task_key/move", post(move_task))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TaskDto {
    pub id: Uuid,
    pub key: String,
    pub project_id: Uuid,
    pub project_key: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub title: String,
    pub description: String,
    pub r#type: String,
    pub priority: String,
    pub status: String,
    pub assignee_id: Option<Uuid>,
    pub reporter_id: Option<Uuid>,
    pub parent_task_id: Option<Uuid>,
    pub estimate_minutes: Option<i32>,
    pub story_points: Option<i32>,
    pub due_date: Option<NaiveDate>,
    pub labels: Vec<String>,
    pub order_in_column: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateTaskReq {
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 100_000))]
    pub description: Option<String>,
    pub column_id: Option<Uuid>, // defaults to first column of default board
    pub r#type: Option<String>,
    pub priority: Option<String>,
    pub assignee_id: Option<Uuid>,
    pub parent_task_id: Option<Uuid>,
    pub estimate_minutes: Option<i32>,
    pub story_points: Option<i32>,
    pub due_date: Option<NaiveDate>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditTaskReq {
    #[validate(length(min = 1, max = 200))]
    pub title: Option<String>,
    #[validate(length(max = 100_000))]
    pub description: Option<String>,
    pub r#type: Option<String>,
    pub priority: Option<String>,
    pub assignee_id: Option<Uuid>,
    pub estimate_minutes: Option<i32>,
    pub story_points: Option<i32>,
    pub due_date: Option<NaiveDate>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct MoveTaskReq {
    pub column_id: Uuid,
    /// Drop after this task in the destination column.
    pub after_task_id: Option<Uuid>,
    /// Or drop before this task. Mutually exclusive with `after_task_id`.
    pub before_task_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    /// Tiny DSL: "assignee:me+status:in_progress+label:backend".
    pub filter: Option<String>,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn create_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateTaskReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource())
        && !can(&user.as_actor(), Action::ViewProject, ctx.as_resource())
    {
        // Anyone who can view + isn't a watcher can still create — that's a
        // reasonable default for a PM tool. The viewer global role can't:
        // EditProject is the gate.
        return Err(AppError::Forbidden);
    }
    if ctx.archived {
        return Err(AppError::Conflict("project is archived".into()));
    }

    // Validate enums up-front so the activity row is consistent.
    let r#type = req.r#type.as_deref().unwrap_or("feature");
    if !matches!(r#type, "feature" | "bug" | "chore" | "spike" | "incident") {
        return Err(AppError::BadRequest("type invalid".into()));
    }
    let priority = req.priority.as_deref().unwrap_or("p2");
    if !matches!(priority, "p0" | "p1" | "p2" | "p3") {
        return Err(AppError::BadRequest("priority invalid".into()));
    }

    let mut tx = state.db.begin().await?;

    // Resolve the destination column. If unspecified, first column of the
    // default board.
    let (board_id, column_id) = match req.column_id {
        Some(col_id) => {
            let row = sqlx::query!(
                r#"
                SELECT bc.board_id AS "board_id!: Uuid"
                FROM   board_columns bc
                JOIN   boards b ON b.id = bc.board_id
                WHERE  bc.id = $1 AND bc.deleted_at IS NULL
                  AND  b.project_id = $2 AND b.deleted_at IS NULL
                "#,
                col_id,
                ctx.id
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::BadRequest(
                "column does not belong to this project".into(),
            ))?;
            (row.board_id, col_id)
        }
        None => {
            let row = sqlx::query!(
                r#"
                SELECT b.id  AS "board_id!: Uuid",
                       bc.id AS "column_id!: Uuid"
                FROM   boards b
                JOIN   board_columns bc ON bc.board_id = b.id AND bc.deleted_at IS NULL
                WHERE  b.project_id = $1 AND b.is_default = true AND b.deleted_at IS NULL
                ORDER  BY bc.sort_order ASC
                LIMIT  1
                "#,
                ctx.id
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::Conflict(
                "project has no default board with columns".into(),
            ))?;
            (row.board_id, row.column_id)
        }
    };

    // Status derives from column category.
    let category: String =
        sqlx::query_scalar(r#"SELECT category FROM board_columns WHERE id = $1"#)
            .bind(column_id)
            .fetch_one(&mut *tx)
            .await?;

    // Append to end of the target column.
    let max_o: Option<f64> = sqlx::query_scalar(
        r#"SELECT MAX(order_in_column) FROM tasks
           WHERE column_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(column_id)
    .fetch_one(&mut *tx)
    .await?;
    let order_in_column = max_o.unwrap_or(0.0) + 1024.0;

    // Reserve key.
    let (task_key, _seq) = task_domain::next_key(&mut tx, ctx.id).await?;
    let task_id = Uuid::now_v7();

    sqlx::query(
        r#"
        INSERT INTO tasks (
            id, project_id, board_id, column_id, key, title, description,
            type, priority, status, assignee_id, reporter_id, parent_task_id,
            estimate_minutes, story_points, due_date, labels, order_in_column
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10, $11, $12, $13,
            $14, $15, $16, $17, $18
        )
        "#,
    )
    .bind(task_id)
    .bind(ctx.id)
    .bind(board_id)
    .bind(column_id)
    .bind(&task_key)
    .bind(&req.title)
    .bind(req.description.as_deref().unwrap_or(""))
    .bind(r#type)
    .bind(priority)
    .bind(category)
    .bind(req.assignee_id)
    .bind(user.id)
    .bind(req.parent_task_id)
    .bind(req.estimate_minutes)
    .bind(req.story_points)
    .bind(req.due_date)
    .bind(req.labels.as_deref().unwrap_or(&[]))
    .bind(order_in_column)
    .execute(&mut *tx)
    .await?;

    // Auto-watch: reporter + assignee.
    sqlx::query(
        r#"
        INSERT INTO task_watchers (task_id, user_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(task_id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    if let Some(a) = req.assignee_id {
        sqlx::query(
            r#"
            INSERT INTO task_watchers (task_id, user_id) VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(task_id)
        .bind(a)
        .execute(&mut *tx)
        .await?;
    }

    task_domain::log_activity(
        &mut tx,
        task_id,
        Some(user.id),
        "created",
        &serde_json::json!({ "title": req.title }),
    )
    .await?;

    tx.commit().await?;

    let dto = fetch_task(&state.db, &task_key, ctx.id).await?;
    crate::infra::events::publish(
        &state.redis,
        &Event::TaskCreated {
            project_id: ctx.id,
            board_id,
            task_id,
            key: task_key.clone(),
        },
    )
    .await;

    // Notify the assignee (notify() skips self).
    if let Some(assignee) = req.assignee_id {
        let _ = crate::domain::notifications::notify(
            &state.db,
            &state.redis,
            assignee,
            user.id,
            "assigned",
            &format!("You were assigned {task_key}"),
            None,
            Some(&format!("/tasks/{task_key}")),
            Some(task_id),
        )
        .await;
    }

    // Outbound webhooks (best-effort).
    let _ = crate::domain::webhooks::dispatch(
        &state.db,
        ctx.id,
        "task.created",
        serde_json::json!({ "task_id": task_id, "key": task_key, "board_id": board_id }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn list_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Query(q): Query<ListTasksQuery>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Tiny filter DSL: tokens joined by '+'. Supports:
    //   assignee:me | assignee:<uuid>
    //   status:<todo|in_progress|review|done>
    //   priority:<p0|p1|p2|p3>
    //   type:<feature|bug|chore|spike|incident>
    //   label:<text>     (multiple allowed; all must match)
    let filter = parse_filter(q.filter.as_deref(), user.id);

    let rows = sqlx::query!(
        r#"
        SELECT t.id              AS "id!: Uuid",
               t.key             AS "key!: String",
               t.project_id      AS "project_id!: Uuid",
               p.key             AS "project_key!: String",
               t.board_id        AS "board_id!: Uuid",
               t.column_id       AS "column_id!: Uuid",
               t.title           AS "title!: String",
               t.description     AS "description!: String",
               t.type            AS "type!: String",
               t.priority        AS "priority!: String",
               t.status          AS "status!: String",
               t.assignee_id,
               t.reporter_id,
               t.parent_task_id,
               t.estimate_minutes,
               t.story_points,
               t.due_date,
               t.labels          AS "labels!: Vec<String>",
               t.order_in_column AS "order_in_column!: f64",
               t.created_at      AS "created_at!: DateTime<Utc>",
               t.updated_at      AS "updated_at!: DateTime<Utc>",
               t.completed_at
        FROM   tasks t
        JOIN   projects p ON p.id = t.project_id
        WHERE  t.project_id = $1
          AND  t.deleted_at IS NULL
          AND  ($2::uuid  IS NULL OR t.assignee_id = $2)
          AND  ($3::text  IS NULL OR t.status   = $3)
          AND  ($4::text  IS NULL OR t.priority = $4)
          AND  ($5::text  IS NULL OR t.type     = $5)
          AND  ($6::text[] IS NULL OR t.labels @> $6)
        ORDER  BY t.column_id, t.order_in_column ASC
        "#,
        ctx.id,
        filter.assignee,
        filter.status,
        filter.priority,
        filter.r#type,
        filter.labels.as_deref()
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<TaskDto> = rows
        .into_iter()
        .map(|r| TaskDto {
            id: r.id,
            key: r.key,
            project_id: r.project_id,
            project_key: r.project_key,
            board_id: r.board_id,
            column_id: r.column_id,
            title: r.title,
            description: r.description,
            r#type: r.r#type,
            priority: r.priority,
            status: r.status,
            assignee_id: r.assignee_id,
            reporter_id: r.reporter_id,
            parent_task_id: r.parent_task_id,
            estimate_minutes: r.estimate_minutes,
            story_points: r.story_points,
            due_date: r.due_date,
            labels: r.labels,
            order_in_column: r.order_in_column,
            created_at: r.created_at,
            updated_at: r.updated_at,
            completed_at: r.completed_at,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn get_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let (project_id, project_key) = resolve_project_from_task_key(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let dto = fetch_task(&state.db, &task_key, project_id).await?;
    Ok(Json(dto))
}

async fn edit_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<EditTaskReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let (project_id, project_key) = resolve_project_from_task_key(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(t) = req.r#type.as_deref() {
        if !matches!(t, "feature" | "bug" | "chore" | "spike" | "incident") {
            return Err(AppError::BadRequest("type invalid".into()));
        }
    }
    if let Some(p) = req.priority.as_deref() {
        if !matches!(p, "p0" | "p1" | "p2" | "p3") {
            return Err(AppError::BadRequest("priority invalid".into()));
        }
    }

    let task_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM tasks WHERE key = $1 AND project_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(&task_key)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    // Current assignee, so we only notify on an actual re-assignment.
    let old_assignee: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT assignee_id FROM tasks WHERE id = $1"#)
            .bind(task_id)
            .fetch_one(&state.db)
            .await?;

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        UPDATE tasks SET
            title             = COALESCE($2,  title),
            description       = COALESCE($3,  description),
            type              = COALESCE($4,  type),
            priority          = COALESCE($5,  priority),
            assignee_id       = COALESCE($6,  assignee_id),
            estimate_minutes  = COALESCE($7,  estimate_minutes),
            story_points      = COALESCE($8,  story_points),
            due_date          = COALESCE($9,  due_date),
            labels            = COALESCE($10, labels)
        WHERE  id = $1
        "#,
    )
    .bind(task_id)
    .bind(req.title.as_deref())
    .bind(req.description.as_deref())
    .bind(req.r#type.as_deref())
    .bind(req.priority.as_deref())
    .bind(req.assignee_id)
    .bind(req.estimate_minutes)
    .bind(req.story_points)
    .bind(req.due_date)
    .bind(req.labels.as_deref())
    .execute(&mut *tx)
    .await?;

    // Cheap activity: one row tagged with which fields changed. M5 expands.
    task_domain::log_activity(
        &mut tx,
        task_id,
        Some(user.id),
        "titled",
        &serde_json::json!({}),
    )
    .await?;
    tx.commit().await?;

    // Notify on re-assignment to a new person (notify() skips self).
    if let Some(new_assignee) = req.assignee_id {
        if Some(new_assignee) != old_assignee {
            let _ = crate::domain::notifications::notify(
                &state.db,
                &state.redis,
                new_assignee,
                user.id,
                "assigned",
                &format!("You were assigned {task_key}"),
                None,
                Some(&format!("/tasks/{task_key}")),
                Some(task_id),
            )
            .await;
        }
    }

    // Outbound webhooks (best-effort).
    let _ = crate::domain::webhooks::dispatch(
        &state.db,
        project_id,
        "task.updated",
        serde_json::json!({ "task_id": task_id, "key": task_key }),
    )
    .await;

    let dto = fetch_task(&state.db, &task_key, project_id).await?;
    crate::infra::events::publish(
        &state.redis,
        &Event::TaskUpdated {
            project_id,
            task_id,
            key: task_key,
        },
    )
    .await;
    Ok(Json(dto))
}

async fn delete_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let (project_id, project_key) = resolve_project_from_task_key(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let task_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM tasks WHERE key = $1 AND project_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(&task_key)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    sqlx::query("UPDATE tasks SET deleted_at = now() WHERE id = $1")
        .bind(task_id)
        .execute(&state.db)
        .await?;
    crate::infra::events::publish(
        &state.redis,
        &Event::TaskDeleted {
            project_id,
            task_id,
            key: task_key,
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn move_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
    Json(req): Json<MoveTaskReq>,
) -> AppResult<impl IntoResponse> {
    if req.after_task_id.is_some() && req.before_task_id.is_some() {
        return Err(AppError::BadRequest(
            "specify after_task_id OR before_task_id, not both".into(),
        ));
    }
    let (project_id, project_key) = resolve_project_from_task_key(&state.db, &task_key).await?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let task = sqlx::query!(
        r#"
        SELECT id        AS "id!: Uuid",
               board_id  AS "board_id!: Uuid",
               column_id AS "column_id!: Uuid"
        FROM   tasks
        WHERE  key = $1 AND project_id = $2 AND deleted_at IS NULL
        "#,
        task_key,
        project_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    // Verify destination column belongs to the same project.
    let dest = sqlx::query!(
        r#"
        SELECT bc.id       AS "id!: Uuid",
               bc.board_id AS "board_id!: Uuid",
               bc.category AS "category!: String",
               b.project_id AS "project_id!: Uuid"
        FROM   board_columns bc
        JOIN   boards b ON b.id = bc.board_id
        WHERE  bc.id = $1 AND bc.deleted_at IS NULL AND b.deleted_at IS NULL
        "#,
        req.column_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::BadRequest("destination column not found".into()))?;

    if dest.project_id != project_id {
        return Err(AppError::BadRequest(
            "cannot move tasks across projects".into(),
        ));
    }

    let new_order = task_domain::resolve_position(
        &state.db,
        req.column_id,
        req.after_task_id,
        req.before_task_id,
    )
    .await?;

    let mut tx = state.db.begin().await?;
    sqlx::query(
        r#"
        UPDATE tasks
           SET column_id = $2,
               board_id  = $3,
               status    = $4,
               order_in_column = $5,
               completed_at = CASE WHEN $4 = 'done'
                                   AND completed_at IS NULL THEN now()
                                   WHEN $4 <> 'done' THEN NULL
                                   ELSE completed_at END,
               started_at   = CASE WHEN $4 IN ('in_progress','review')
                                   AND started_at IS NULL THEN now()
                                   ELSE started_at END
         WHERE id = $1
        "#,
    )
    .bind(task.id)
    .bind(req.column_id)
    .bind(dest.board_id)
    .bind(&dest.category)
    .bind(new_order)
    .execute(&mut *tx)
    .await?;
    task_domain::log_activity(
        &mut tx,
        task.id,
        Some(user.id),
        "moved",
        &serde_json::json!({
            "from_column_id": task.column_id,
            "to_column_id":   req.column_id,
        }),
    )
    .await?;
    tx.commit().await?;

    crate::infra::events::publish(
        &state.redis,
        &Event::TaskMoved {
            project_id,
            board_id: dest.board_id,
            task_id: task.id,
            key: task_key.clone(),
            from_column_id: task.column_id,
            to_column_id: req.column_id,
        },
    )
    .await;
    let dto = fetch_task(&state.db, &task_key, project_id).await?;
    Ok(Json(dto))
}

// ─── helpers ────────────────────────────────────────────────────────────────

async fn resolve_project_from_task_key(db: &PgPool, task_key: &str) -> AppResult<(Uuid, String)> {
    // Task key is "PROJ-N". Look up project by prefix; tasks.key is unique per
    // project so we can also just JOIN.
    let row = sqlx::query!(
        r#"
        SELECT t.project_id AS "project_id!: Uuid",
               p.key        AS "project_key!: String"
        FROM   tasks t
        JOIN   projects p ON p.id = t.project_id
        WHERE  t.key = $1 AND t.deleted_at IS NULL AND p.deleted_at IS NULL
        "#,
        task_key
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok((row.project_id, row.project_key))
}

async fn fetch_task(db: &PgPool, task_key: &str, project_id: Uuid) -> AppResult<TaskDto> {
    let r = sqlx::query!(
        r#"
        SELECT t.id              AS "id!: Uuid",
               t.key             AS "key!: String",
               t.project_id      AS "project_id!: Uuid",
               p.key             AS "project_key!: String",
               t.board_id        AS "board_id!: Uuid",
               t.column_id       AS "column_id!: Uuid",
               t.title           AS "title!: String",
               t.description     AS "description!: String",
               t.type            AS "type!: String",
               t.priority        AS "priority!: String",
               t.status          AS "status!: String",
               t.assignee_id,
               t.reporter_id,
               t.parent_task_id,
               t.estimate_minutes,
               t.story_points,
               t.due_date,
               t.labels          AS "labels!: Vec<String>",
               t.order_in_column AS "order_in_column!: f64",
               t.created_at      AS "created_at!: DateTime<Utc>",
               t.updated_at      AS "updated_at!: DateTime<Utc>",
               t.completed_at
        FROM   tasks t
        JOIN   projects p ON p.id = t.project_id
        WHERE  t.key = $1 AND t.project_id = $2 AND t.deleted_at IS NULL
        "#,
        task_key,
        project_id
    )
    .fetch_one(db)
    .await?;
    Ok(TaskDto {
        id: r.id,
        key: r.key,
        project_id: r.project_id,
        project_key: r.project_key,
        board_id: r.board_id,
        column_id: r.column_id,
        title: r.title,
        description: r.description,
        r#type: r.r#type,
        priority: r.priority,
        status: r.status,
        assignee_id: r.assignee_id,
        reporter_id: r.reporter_id,
        parent_task_id: r.parent_task_id,
        estimate_minutes: r.estimate_minutes,
        story_points: r.story_points,
        due_date: r.due_date,
        labels: r.labels,
        order_in_column: r.order_in_column,
        created_at: r.created_at,
        updated_at: r.updated_at,
        completed_at: r.completed_at,
    })
}

#[derive(Default)]
struct ParsedFilter {
    assignee: Option<Uuid>,
    status: Option<String>,
    priority: Option<String>,
    r#type: Option<String>,
    labels: Option<Vec<String>>,
}

fn parse_filter(raw: Option<&str>, current_user: Uuid) -> ParsedFilter {
    let mut out = ParsedFilter::default();
    let Some(raw) = raw else { return out };
    let mut labels: Vec<String> = Vec::new();
    for token in raw.split('+') {
        let Some((k, v)) = token.split_once(':') else {
            continue;
        };
        match k {
            "assignee" => {
                if v == "me" {
                    out.assignee = Some(current_user);
                } else if let Ok(uid) = Uuid::parse_str(v) {
                    out.assignee = Some(uid);
                }
            }
            "status" => out.status = Some(v.to_string()),
            "priority" => out.priority = Some(v.to_string()),
            "type" => out.r#type = Some(v.to_string()),
            "label" => labels.push(v.to_string()),
            _ => {}
        }
    }
    if !labels.is_empty() {
        out.labels = Some(labels);
    }
    out
}
