//! Sprint endpoints.
//!
//!   POST   /projects/:key/sprints            — create (planned)
//!   GET    /projects/:key/sprints            — list (newest first)
//!   GET    /sprints/:id                      — detail
//!   PATCH  /sprints/:id                      — edit name/goal/dates (dates while
//!                                              planned or active), retro summary
//!   DELETE /sprints/:id                      — soft delete; its tasks fall back
//!                                              to the backlog, never deleted
//!   POST   /sprints/:id/start                — planned → active (kicks off WS)
//!   POST   /sprints/:id/complete             — active → completed, opens retro,
//!                                              snapshots velocity_points, and
//!                                              optionally carries unfinished
//!                                              work to the backlog / another
//!                                              sprint / a brand-new one
//!   POST   /sprints/:id/tasks/:task_key      — assign task to sprint
//!   DELETE /sprints/:id/tasks/:task_key      — unassign
//!   GET    /sprints/:id/tasks                — list tasks in sprint
//!   GET    /sprints/:id/burndown             — series for the chart

use axum::{
    extract::{Path, State},
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
        projects as project_ctx, sprints as sprint_domain,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/sprints", post(create).get(list_for_project))
        .route(
            "/sprints/:id",
            get(detail).patch(edit).delete(delete_sprint),
        )
        .route("/sprints/:id/start", post(start))
        .route("/sprints/:id/complete", post(complete))
        .route(
            "/sprints/:id/tasks/:task_key",
            post(assign_task).delete(unassign_task),
        )
        .route("/sprints/:id/tasks", get(list_tasks))
        .route("/sprints/:id/burndown", get(burndown))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SprintDto {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_key: String,
    pub name: String,
    pub goal: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub state: String,
    pub velocity_points: Option<i32>,
    pub summary_md: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub total_points: i64,
    pub done_points: i64,
    pub task_count: i64,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateSprintReq {
    #[validate(length(min = 1, max = 80))]
    pub name: String,
    #[validate(length(max = 4000))]
    pub goal: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditSprintReq {
    #[validate(length(min = 1, max = 80))]
    pub name: Option<String>,
    #[validate(length(max = 4000))]
    pub goal: Option<String>,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    /// The retro summary — editable only once the sprint is completed (the
    /// generated markdown is a starting point, not scripture).
    #[validate(length(min = 1, max = 20000))]
    pub summary_md: Option<String>,
}

/// Where a completing sprint's unfinished work goes. Absent = leave it in
/// the completed sprint (the old behaviour).
#[derive(Debug, Deserialize)]
#[serde(tag = "to", rename_all = "snake_case")]
pub enum CarryOver {
    /// Sprint-less: back to the pile.
    Backlog,
    /// An existing planned/active sprint in the same project.
    Sprint { sprint_id: Uuid },
    /// Spin up a fresh sprint and move the work there.
    NewSprint {
        name: String,
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
    },
}

#[derive(Debug, Deserialize, Default)]
pub struct CompleteSprintReq {
    pub carry_over: Option<CarryOver>,
}

#[derive(Debug, Serialize)]
pub struct CompleteSprintResp {
    pub sprint: SprintDto,
    /// How many unfinished tasks moved (0 when nothing was carried over).
    pub carried_over: i64,
    /// The sprint they landed in — set for both `sprint` and `new_sprint`.
    pub carried_to: Option<SprintRef>,
}

#[derive(Debug, Serialize)]
pub struct SprintRef {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct BurndownPointDto {
    pub date: NaiveDate,
    pub remaining_points: i64,
    pub ideal_points: f64,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateSprintReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    if req.ends_at <= req.starts_at {
        return Err(AppError::Validation(
            "ends_at must be after starts_at".into(),
        ));
    }
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO sprints (id, project_id, name, goal, starts_at, ends_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(ctx.id)
    .bind(&req.name)
    .bind(req.goal.as_deref().unwrap_or(""))
    .bind(req.starts_at)
    .bind(req.ends_at)
    .execute(&state.db)
    .await?;
    let dto = fetch_sprint(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn list_for_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let rows = sqlx::query!(
        r#"
        SELECT s.id              AS "id!: Uuid",
               s.project_id      AS "project_id!: Uuid",
               p.key             AS "project_key!: String",
               s.name            AS "name!: String",
               s.goal            AS "goal!: String",
               s.starts_at       AS "starts_at!: DateTime<Utc>",
               s.ends_at         AS "ends_at!: DateTime<Utc>",
               s.state           AS "state!: String",
               s.velocity_points,
               s.summary_md,
               s.started_at,
               s.completed_at,
               COALESCE(t.total_points, 0)  AS "total_points!: i64",
               COALESCE(t.done_points, 0)   AS "done_points!: i64",
               COALESCE(t.task_count, 0)    AS "task_count!: i64"
        FROM   sprints s
        JOIN   projects p ON p.id = s.project_id
        LEFT JOIN LATERAL (
            SELECT  COUNT(*)                                   AS task_count,
                    COALESCE(SUM(story_points), 0)             AS total_points,
                    COALESCE(SUM(CASE WHEN status = 'done'
                                      THEN story_points END), 0) AS done_points
            FROM    tasks
            WHERE   sprint_id = s.id AND deleted_at IS NULL
              AND   parent_task_id IS NULL  -- count top-level tasks only (subtasks roll up under them)
        ) t ON TRUE
        WHERE  s.project_id = $1 AND s.deleted_at IS NULL
        ORDER  BY
            CASE s.state WHEN 'active' THEN 0 WHEN 'planned' THEN 1 ELSE 2 END,
            s.starts_at DESC
        "#,
        ctx.id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<SprintDto> = rows
        .into_iter()
        .map(|r| SprintDto {
            id: r.id,
            project_id: r.project_id,
            project_key: r.project_key,
            name: r.name,
            goal: r.goal,
            starts_at: r.starts_at,
            ends_at: r.ends_at,
            state: r.state,
            velocity_points: r.velocity_points,
            summary_md: r.summary_md,
            started_at: r.started_at,
            completed_at: r.completed_at,
            total_points: r.total_points,
            done_points: r.done_points,
            task_count: r.task_count,
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn detail(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let dto = fetch_sprint(&state.db, id).await?;
    Ok(Json(dto))
}

async fn edit(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<EditSprintReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    // We allow edits during 'planned' and 'active' (name/goal); date edits
    // require 'planned'. The retro summary is the inverse: it only exists
    // once the sprint completes, so that's the only state where editing it
    // makes sense.
    let cur_state: String = sqlx::query_scalar("SELECT state FROM sprints WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let touches_meta = req.name.is_some()
        || req.goal.is_some()
        || req.starts_at.is_some()
        || req.ends_at.is_some();
    if cur_state == "completed" && touches_meta {
        return Err(AppError::Conflict("sprint is completed".into()));
    }
    if req.summary_md.is_some() && cur_state != "completed" {
        return Err(AppError::Conflict(
            "no summary to edit until the sprint completes".into(),
        ));
    }
    // Dates stay editable while the sprint is planned *or* active — moving a
    // running sprint's end date is routine. Completed sprints stay frozen
    // (guarded above) so history and velocity keep meaning what they said.
    if (req.starts_at.is_some() || req.ends_at.is_some()) && cur_state == "completed" {
        return Err(AppError::Conflict(
            "can't change the dates of a completed sprint".into(),
        ));
    }
    sqlx::query(
        r#"
        UPDATE sprints SET
            name       = COALESCE($2, name),
            goal       = COALESCE($3, goal),
            starts_at  = COALESCE($4, starts_at),
            ends_at    = COALESCE($5, ends_at),
            summary_md = COALESCE($6, summary_md)
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(req.name)
    .bind(req.goal)
    .bind(req.starts_at)
    .bind(req.ends_at)
    .bind(req.summary_md)
    .execute(&state.db)
    .await?;
    let dto = fetch_sprint(&state.db, id).await?;
    Ok(Json(dto))
}

async fn start(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let cur_state: String = sqlx::query_scalar("SELECT state FROM sprints WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let cur = sprint_domain::SprintState::parse(&cur_state)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("unknown state")))?;
    let next = sprint_domain::next_state(cur, "start").map_err(|m| AppError::Conflict(m.into()))?;
    // The partial unique index "one active per project" backstops a concurrent
    // start race; we translate the violation to a clean 409.
    let upd = sqlx::query(r#"UPDATE sprints SET state = $2, started_at = now() WHERE id = $1"#)
        .bind(id)
        .bind(next.as_str())
        .execute(&state.db)
        .await;
    if let Err(sqlx::Error::Database(e)) = &upd {
        if e.is_unique_violation() {
            return Err(AppError::Conflict(
                "another sprint is already active".into(),
            ));
        }
    }
    upd?;
    let dto = fetch_sprint(&state.db, id).await?;
    Ok(Json(dto))
}

async fn complete(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    // Optional body: older clients (and the CLI) POST nothing at all.
    body: Option<Json<CompleteSprintReq>>,
) -> AppResult<impl IntoResponse> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let cur_state: String = sqlx::query_scalar("SELECT state FROM sprints WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let cur = sprint_domain::SprintState::parse(&cur_state)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("unknown state")))?;
    let next =
        sprint_domain::next_state(cur, "complete").map_err(|m| AppError::Conflict(m.into()))?;

    let mut tx = state.db.begin().await?;
    // Snapshot velocity.
    let velocity: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(story_points), 0)
           FROM   tasks
           WHERE  sprint_id = $1 AND status = 'done' AND deleted_at IS NULL
             AND  parent_task_id IS NULL"#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        UPDATE sprints SET
            state = $2,
            completed_at = now(),
            velocity_points = $3
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(next.as_str())
    .bind(velocity as i32)
    .execute(&mut *tx)
    .await?;
    // Open the retro (1-to-1 via UNIQUE).
    sqlx::query(
        r#"INSERT INTO sprint_retros (id, sprint_id, state) VALUES ($1, $2, 'open')
           ON CONFLICT (sprint_id) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Freeze the sprint's contents *before* anything moves. This is what the
    // completed sprint's page shows from now on — the Jira model QA asked
    // for — so carrying work forward stops rewriting the sprint it came from.
    // Time is the minutes logged on each task while this sprint ran, not the
    // task's lifetime total.
    sqlx::query(
        r#"
        INSERT INTO sprint_task_snapshots
            (sprint_id, task_id, key, title, status, priority, type, story_points,
             assignee_id, logged_minutes)
        SELECT t.sprint_id, t.id, t.key, t.title, t.status, t.priority, t.type,
               t.story_points, t.assignee_id,
               COALESCE((
                   SELECT SUM(l.duration_minutes)::int
                   FROM   time_logs l
                   WHERE  l.task_id = t.id
                     AND  l.deleted_at IS NULL
                     AND  l.ended_at IS NOT NULL
                     AND  l.started_at >= COALESCE(s.started_at, s.starts_at)
               ), 0)
        FROM   tasks t
        JOIN   sprints s ON s.id = t.sprint_id
        WHERE  t.sprint_id = $1 AND t.deleted_at IS NULL
        ON CONFLICT (sprint_id, task_id) DO NOTHING
        "#,
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Carry the unfinished work somewhere useful. Velocity was snapshotted
    // above from done tasks only, so moving the rest can't skew it.
    let mut carried_to: Option<SprintRef> = None;
    let mut carried_over: i64 = 0;
    if let Some(carry) = req.carry_over {
        let target: Option<Uuid> = match carry {
            CarryOver::Backlog => None,
            CarryOver::Sprint { sprint_id } => {
                if sprint_id == id {
                    return Err(AppError::BadRequest(
                        "can't carry work into the sprint you're completing".into(),
                    ));
                }
                let row = sqlx::query!(
                    r#"
                    SELECT name       AS "name!: String",
                           state      AS "state!: String",
                           project_id AS "project_id!: Uuid"
                    FROM   sprints WHERE id = $1 AND deleted_at IS NULL
                    "#,
                    sprint_id
                )
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(AppError::NotFound)?;
                if row.project_id != project_id {
                    return Err(AppError::BadRequest(
                        "that sprint belongs to another project".into(),
                    ));
                }
                if row.state == "completed" {
                    return Err(AppError::Conflict(
                        "that sprint is already completed — pick another".into(),
                    ));
                }
                carried_to = Some(SprintRef {
                    id: sprint_id,
                    name: row.name,
                });
                Some(sprint_id)
            }
            CarryOver::NewSprint {
                name,
                starts_at,
                ends_at,
            } => {
                let name = name.trim().to_string();
                if name.is_empty() || name.chars().count() > 80 {
                    return Err(AppError::Validation(
                        "sprint name must be 1-80 chars".into(),
                    ));
                }
                if ends_at <= starts_at {
                    return Err(AppError::Validation(
                        "ends_at must be after starts_at".into(),
                    ));
                }
                let new_id = Uuid::now_v7();
                sqlx::query(
                    r#"
                    INSERT INTO sprints (id, project_id, name, goal, starts_at, ends_at)
                    VALUES ($1, $2, $3, '', $4, $5)
                    "#,
                )
                .bind(new_id)
                .bind(project_id)
                .bind(&name)
                .bind(starts_at)
                .bind(ends_at)
                .execute(&mut *tx)
                .await?;
                carried_to = Some(SprintRef { id: new_id, name });
                Some(new_id)
            }
        };
        // Everything not done — subtasks included, they follow their own row.
        let moved = sqlx::query(
            r#"
            UPDATE tasks SET sprint_id = $2
             WHERE sprint_id = $1 AND status <> 'done' AND deleted_at IS NULL
            "#,
        )
        .bind(id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
        carried_over = moved.rows_affected() as i64;
    }
    tx.commit().await?;

    let dto = fetch_sprint(&state.db, id).await?;
    Ok(Json(CompleteSprintResp {
        sprint: dto,
        carried_over,
        carried_to,
    }))
}

/// Soft-delete a sprint. Its tasks are NOT deleted — they fall back to the
/// backlog, so deleting a mis-created sprint can't lose work.
async fn delete_sprint(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ManageBoards, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let mut tx = state.db.begin().await?;
    sqlx::query(r#"UPDATE tasks SET sprint_id = NULL WHERE sprint_id = $1"#)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"UPDATE sprints SET deleted_at = now() WHERE id = $1"#)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, task_key)): Path<(Uuid, String)>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let updated = sqlx::query(
        r#"
        UPDATE tasks SET sprint_id = $1
        WHERE key = $2 AND project_id = $3 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(&task_key)
    .bind(project_id)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn unassign_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, task_key)): Path<(Uuid, String)>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        UPDATE tasks SET sprint_id = NULL
        WHERE key = $1 AND project_id = $2 AND sprint_id = $3
        "#,
    )
    .bind(&task_key)
    .bind(project_id)
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_tasks(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    // A completed sprint answers from its snapshot: the tasks as they stood
    // when it closed, including any since carried elsewhere or deleted. Older
    // sprints completed before snapshots existed have no rows and fall through
    // to the live query, which is all they ever had.
    let snap = sqlx::query!(
        r#"
        SELECT n.key            AS "key!: String",
               n.title          AS "title!: String",
               n.status         AS "status!: String",
               n.priority       AS "priority!: String",
               n.type           AS "type!: String",
               n.story_points,
               n.assignee_id,
               n.logged_minutes AS "logged_minutes!: i32",
               n.snapped_at     AS "snapped_at!: DateTime<Utc>",
               (SELECT count(*) FROM tasks c
                 WHERE c.parent_task_id = n.task_id AND c.deleted_at IS NULL)
                                AS "subtask_count!: i64"
        FROM   sprint_task_snapshots n
        JOIN   sprints s ON s.id = n.sprint_id
        WHERE  n.sprint_id = $1 AND s.state = 'completed'
        ORDER  BY n.status, n.priority, n.title
        "#,
        id
    )
    .fetch_all(&state.db)
    .await?;
    if !snap.is_empty() {
        let snapped_at = snap[0].snapped_at;
        let items: Vec<_> = snap
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "key": r.key,
                    "title": r.title,
                    "status": r.status,
                    "priority": r.priority,
                    "type": r.r#type,
                    "story_points": r.story_points,
                    "assignee_id": r.assignee_id,
                    "subtask_count": r.subtask_count,
                    "logged_minutes": r.logged_minutes,
                })
            })
            .collect();
        return Ok(Json(serde_json::json!({
            "items": items,
            "snapshot": true,
            "snapped_at": snapped_at,
        })));
    }

    let rows = sqlx::query!(
        r#"
        SELECT t.key          AS "key!: String",
               t.title        AS "title!: String",
               t.status       AS "status!: String",
               t.priority     AS "priority!: String",
               t.type         AS "type!: String",
               t.story_points,
               t.assignee_id,
               (SELECT count(*) FROM tasks c
                 WHERE c.parent_task_id = t.id AND c.deleted_at IS NULL)
                              AS "subtask_count!: i64"
        FROM   tasks t
        WHERE  t.sprint_id = $1 AND t.deleted_at IS NULL
        ORDER  BY t.status, t.priority, t.updated_at DESC
        "#,
        id
    )
    .fetch_all(&state.db)
    .await?;
    let items: Vec<_> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "key": r.key,
                "title": r.title,
                "status": r.status,
                "priority": r.priority,
                "type": r.r#type,
                "story_points": r.story_points,
                "assignee_id": r.assignee_id,
                "subtask_count": r.subtask_count,
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "items": items, "snapshot": false }),
    ))
}

async fn burndown(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id = project_of_sprint(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let sprint = sqlx::query!(
        r#"
        SELECT starts_at  AS "starts_at!: DateTime<Utc>",
               ends_at    AS "ends_at!: DateTime<Utc>"
        FROM   sprints WHERE id = $1 AND deleted_at IS NULL
        "#,
        id
    )
    .fetch_one(&state.db)
    .await?;
    let total_points: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(story_points), 0)
        FROM   tasks
        WHERE  sprint_id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let completions = sqlx::query!(
        r#"
        SELECT completed_at AS "completed_at!: DateTime<Utc>",
               COALESCE(story_points, 0) AS "story_points!: i32"
        FROM   tasks
        WHERE  sprint_id = $1 AND deleted_at IS NULL
          AND  completed_at IS NOT NULL
        ORDER  BY completed_at ASC
        "#,
        id
    )
    .fetch_all(&state.db)
    .await?;
    let comps: Vec<(DateTime<Utc>, i64)> = completions
        .into_iter()
        .map(|r| (r.completed_at, r.story_points as i64))
        .collect();
    let series = sprint_domain::burndown(sprint.starts_at, sprint.ends_at, total_points, &comps);
    let dto: Vec<BurndownPointDto> = series
        .into_iter()
        .map(|p| BurndownPointDto {
            date: p.date,
            remaining_points: p.remaining_points,
            ideal_points: p.ideal_points,
        })
        .collect();
    Ok(Json(serde_json::json!({
        "items": dto,
        "total_points": total_points,
    })))
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

async fn fetch_sprint(db: &PgPool, id: Uuid) -> AppResult<SprintDto> {
    let r = sqlx::query!(
        r#"
        SELECT s.id              AS "id!: Uuid",
               s.project_id      AS "project_id!: Uuid",
               p.key             AS "project_key!: String",
               s.name            AS "name!: String",
               s.goal            AS "goal!: String",
               s.starts_at       AS "starts_at!: DateTime<Utc>",
               s.ends_at         AS "ends_at!: DateTime<Utc>",
               s.state           AS "state!: String",
               s.velocity_points,
               s.summary_md,
               s.started_at,
               s.completed_at,
               COALESCE(t.total_points, 0)  AS "total_points!: i64",
               COALESCE(t.done_points, 0)   AS "done_points!: i64",
               COALESCE(t.task_count, 0)    AS "task_count!: i64"
        FROM   sprints s
        JOIN   projects p ON p.id = s.project_id
        LEFT JOIN LATERAL (
            SELECT  COUNT(*)::bigint                          AS task_count,
                    COALESCE(SUM(story_points), 0)::bigint    AS total_points,
                    COALESCE(SUM(CASE WHEN status = 'done'
                                 THEN story_points END), 0)::bigint AS done_points
            FROM    tasks
            WHERE   sprint_id = s.id AND deleted_at IS NULL
              AND   parent_task_id IS NULL  -- count top-level tasks only (subtasks roll up under them)
        ) t ON TRUE
        WHERE  s.id = $1
        "#,
        id
    )
    .fetch_one(db)
    .await?;
    Ok(SprintDto {
        id: r.id,
        project_id: r.project_id,
        project_key: r.project_key,
        name: r.name,
        goal: r.goal,
        starts_at: r.starts_at,
        ends_at: r.ends_at,
        state: r.state,
        velocity_points: r.velocity_points,
        summary_md: r.summary_md,
        started_at: r.started_at,
        completed_at: r.completed_at,
        total_points: r.total_points,
        done_points: r.done_points,
        task_count: r.task_count,
    })
}
