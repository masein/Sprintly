//! Dashboard reads. Two endpoints, one per surface:
//!
//!   GET /projects/:key/dashboard  — project-scoped overview
//!   GET /me/dashboard             — personal "My day" overview
//!
//! These return one big aggregate JSON each. The frontend fans out to
//! sub-components, but the API stays one round-trip. Each underlying query
//! is bounded (LIMIT, time window) so the response stays predictable.

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::{
        permissions::{can, Action, Role as GlobalRole},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/dashboard", get(project_dashboard))
        .route("/me/dashboard", get(my_dashboard))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ProjectDashboardDto {
    pub status_counts: StatusCounts,
    pub current_sprint: Option<CurrentSprintSummary>,
    pub velocity_history: Vec<VelocityPoint>,
    pub top_contributors: Vec<ContributorRow>,
    pub recent_activity: Vec<ActivityRow>,
    pub blocked: BlockedSummary,
    pub upcoming_due: Vec<DueRow>,
    pub time_this_week_minutes: i64,
}

#[derive(Debug, Serialize, Default)]
pub struct StatusCounts {
    pub todo: i64,
    pub in_progress: i64,
    pub review: i64,
    pub done: i64,
}

#[derive(Debug, Serialize)]
pub struct CurrentSprintSummary {
    pub id: Uuid,
    pub name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub total_points: i64,
    pub done_points: i64,
    pub task_count: i64,
}

#[derive(Debug, Serialize)]
pub struct VelocityPoint {
    pub sprint_id: Uuid,
    pub name: String,
    pub completed_at: Option<DateTime<Utc>>,
    pub velocity_points: i32,
}

#[derive(Debug, Serialize)]
pub struct ContributorRow {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub minutes: i64,
}

#[derive(Debug, Serialize)]
pub struct ActivityRow {
    pub id: Uuid,
    pub task_key: String,
    pub kind: String,
    pub actor_handle: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct BlockedSummary {
    pub count: i64,
    pub samples: Vec<BlockedSample>,
}

#[derive(Debug, Serialize)]
pub struct BlockedSample {
    pub task_key: String,
    pub title: String,
    pub blocked_by_count: i64,
}

#[derive(Debug, Serialize)]
pub struct DueRow {
    pub task_key: String,
    pub title: String,
    pub due_date: NaiveDate,
    pub assignee_handle: Option<String>,
    pub days_until: i64,
}

#[derive(Debug, Serialize)]
pub struct MyDashboardDto {
    pub my_status_counts: StatusCounts,
    pub overdue: Vec<DueRow>,
    pub my_tasks_sample: Vec<MyTaskSample>,
    pub watched_changed_recently: Vec<WatchedRow>,
    pub time_this_week_minutes: i64,
    pub running_timer: Option<RunningTimerRef>,
}

#[derive(Debug, Serialize)]
pub struct MyTaskSample {
    pub key: String,
    pub project_key: String,
    pub title: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Serialize)]
pub struct WatchedRow {
    pub task_key: String,
    pub title: String,
    pub last_activity_at: DateTime<Utc>,
    pub last_kind: String,
}

#[derive(Debug, Serialize)]
pub struct RunningTimerRef {
    pub task_key: String,
    pub started_at: DateTime<Utc>,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn project_dashboard(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let pid = ctx.id;

    // ── status counts ──
    let sc = sqlx::query!(
        r#"
        SELECT
          COUNT(*) FILTER (WHERE status = 'todo')        AS "todo!: i64",
          COUNT(*) FILTER (WHERE status = 'in_progress') AS "in_progress!: i64",
          COUNT(*) FILTER (WHERE status = 'review')      AS "review!: i64",
          COUNT(*) FILTER (WHERE status = 'done')        AS "done!: i64"
        FROM tasks
        WHERE project_id = $1 AND deleted_at IS NULL
        "#,
        pid
    )
    .fetch_one(&state.db)
    .await?;
    let status_counts = StatusCounts {
        todo: sc.todo,
        in_progress: sc.in_progress,
        review: sc.review,
        done: sc.done,
    };

    // ── current sprint ──
    let cur_sprint = sqlx::query!(
        r#"
        SELECT s.id          AS "id!: Uuid",
               s.name        AS "name!: String",
               s.starts_at   AS "starts_at!: DateTime<Utc>",
               s.ends_at     AS "ends_at!: DateTime<Utc>",
               COALESCE((SELECT COUNT(*) FROM tasks t
                          WHERE t.sprint_id = s.id AND t.deleted_at IS NULL
                            AND t.parent_task_id IS NULL), 0)
                             AS "task_count!: i64",
               COALESCE((SELECT SUM(story_points) FROM tasks t
                          WHERE t.sprint_id = s.id AND t.deleted_at IS NULL
                            AND t.parent_task_id IS NULL), 0)::bigint
                             AS "total_points!: i64",
               COALESCE((SELECT SUM(story_points) FROM tasks t
                          WHERE t.sprint_id = s.id AND t.status = 'done'
                            AND t.deleted_at IS NULL
                            AND t.parent_task_id IS NULL), 0)::bigint
                             AS "done_points!: i64"
        FROM   sprints s
        WHERE  s.project_id = $1 AND s.state = 'active' AND s.deleted_at IS NULL
        LIMIT  1
        "#,
        pid
    )
    .fetch_optional(&state.db)
    .await?
    .map(|r| CurrentSprintSummary {
        id: r.id,
        name: r.name,
        starts_at: r.starts_at,
        ends_at: r.ends_at,
        total_points: r.total_points,
        done_points: r.done_points,
        task_count: r.task_count,
    });

    // ── velocity history (last 10 completed) ──
    let vel = sqlx::query!(
        r#"
        SELECT id              AS "id!: Uuid",
               name            AS "name!: String",
               completed_at,
               velocity_points AS "velocity_points!: i32"
        FROM   sprints
        WHERE  project_id = $1 AND state = 'completed'
          AND  velocity_points IS NOT NULL
          AND  deleted_at IS NULL
        ORDER  BY completed_at DESC
        LIMIT  10
        "#,
        pid
    )
    .fetch_all(&state.db)
    .await?;
    let mut velocity_history: Vec<VelocityPoint> = vel
        .into_iter()
        .map(|r| VelocityPoint {
            sprint_id: r.id,
            name: r.name,
            completed_at: r.completed_at,
            velocity_points: r.velocity_points,
        })
        .collect();
    velocity_history.reverse(); // chronological for charts

    // ── top contributors this week (time logged) ──
    let week_start = monday_utc(Utc::now());
    let top = sqlx::query!(
        r#"
        SELECT u.id            AS "user_id!: Uuid",
               u.handle        AS "handle!: String",
               u.display_name  AS "display_name!: String",
               COALESCE(SUM(tl.duration_minutes), 0)::bigint AS "minutes!: i64"
        FROM   users u
        JOIN   time_logs tl ON tl.user_id = u.id
               AND tl.deleted_at IS NULL
               AND tl.ended_at IS NOT NULL
               AND tl.started_at >= $2
        JOIN   tasks t ON t.id = tl.task_id
        WHERE  t.project_id = $1 AND t.deleted_at IS NULL
        GROUP  BY u.id, u.handle, u.display_name
        ORDER  BY 4 DESC
        LIMIT  5
        "#,
        pid,
        week_start
    )
    .fetch_all(&state.db)
    .await?;
    let top_contributors: Vec<ContributorRow> = top
        .into_iter()
        .map(|r| ContributorRow {
            user_id: r.user_id,
            handle: r.handle,
            display_name: r.display_name,
            minutes: r.minutes,
        })
        .collect();

    // ── recent activity (last 20 events for this project's tasks) ──
    let act = sqlx::query!(
        r#"
        SELECT a.id           AS "id!: Uuid",
               t.key          AS "task_key!: String",
               a.kind         AS "kind!: String",
               u.handle       AS "actor_handle?: String",
               a.created_at   AS "created_at!: DateTime<Utc>"
        FROM   task_activity a
        JOIN   tasks t ON t.id = a.task_id AND t.deleted_at IS NULL
        LEFT JOIN users u ON u.id = a.actor_id
        WHERE  t.project_id = $1
        ORDER  BY a.created_at DESC
        LIMIT  20
        "#,
        pid
    )
    .fetch_all(&state.db)
    .await?;
    let recent_activity: Vec<ActivityRow> = act
        .into_iter()
        .map(|r| ActivityRow {
            id: r.id,
            task_key: r.task_key,
            kind: r.kind,
            actor_handle: r.actor_handle,
            created_at: r.created_at,
        })
        .collect();

    // ── blocked tasks ──
    // A task is "blocked" if there's at least one incoming `blocks` link
    // from a task that isn't done yet.
    let blocked_rows = sqlx::query!(
        r#"
        SELECT t.key   AS "key!: String",
               t.title AS "title!: String",
               COUNT(l.from_task_id) AS "blocked_by!: i64"
        FROM   tasks t
        JOIN   task_links l ON l.to_task_id = t.id AND l.kind = 'blocks'
        JOIN   tasks blocker ON blocker.id = l.from_task_id
               AND blocker.status <> 'done'
               AND blocker.deleted_at IS NULL
        WHERE  t.project_id = $1
           AND t.deleted_at IS NULL
           AND t.status <> 'done'
        GROUP  BY t.id, t.key, t.title
        ORDER  BY "blocked_by!: i64" DESC, t.updated_at DESC
        LIMIT  10
        "#,
        pid
    )
    .fetch_all(&state.db)
    .await?;
    let blocked_count = blocked_rows.len() as i64;
    let blocked_samples: Vec<BlockedSample> = blocked_rows
        .into_iter()
        .take(5)
        .map(|r| BlockedSample {
            task_key: r.key,
            title: r.title,
            blocked_by_count: r.blocked_by,
        })
        .collect();

    // ── upcoming due dates (next 14 days, not done) ──
    let today = Utc::now().date_naive();
    let horizon = today + chrono::Duration::days(14);
    let due_rows = sqlx::query!(
        r#"
        SELECT t.key            AS "key!: String",
               t.title          AS "title!: String",
               t.due_date       AS "due_date!: NaiveDate",
               u.handle         AS "assignee_handle?: String"
        FROM   tasks t
        LEFT JOIN users u ON u.id = t.assignee_id
        WHERE  t.project_id = $1
          AND  t.deleted_at IS NULL
          AND  t.status <> 'done'
          AND  t.due_date IS NOT NULL
          AND  t.due_date <= $2
        ORDER  BY t.due_date ASC
        LIMIT  20
        "#,
        pid,
        horizon,
    )
    .fetch_all(&state.db)
    .await?;
    let upcoming_due: Vec<DueRow> = due_rows
        .into_iter()
        .map(|r| {
            let days = (r.due_date - today).num_days();
            DueRow {
                task_key: r.key,
                title: r.title,
                due_date: r.due_date,
                assignee_handle: r.assignee_handle,
                days_until: days,
            }
        })
        .collect();

    // ── time this week (all users, this project) ──
    let time_this_week_minutes: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(tl.duration_minutes), 0)::bigint
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        WHERE  t.project_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2
        "#,
    )
    .bind(pid)
    .bind(week_start)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(ProjectDashboardDto {
        status_counts,
        current_sprint: cur_sprint,
        velocity_history,
        top_contributors,
        recent_activity,
        blocked: BlockedSummary {
            count: blocked_count,
            samples: blocked_samples,
        },
        upcoming_due,
        time_this_week_minutes,
    }))
}

async fn my_dashboard(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let today = Utc::now().date_naive();
    let week_start = monday_utc(Utc::now());

    // ── my status counts (only across accessible projects) ──
    let accessible = accessible_project_ids(&state.db, &user).await?;

    let mut my_status_counts = StatusCounts::default();
    if !accessible.is_empty() {
        let sc = sqlx::query!(
            r#"
            SELECT
              COUNT(*) FILTER (WHERE status = 'todo')        AS "todo!: i64",
              COUNT(*) FILTER (WHERE status = 'in_progress') AS "in_progress!: i64",
              COUNT(*) FILTER (WHERE status = 'review')      AS "review!: i64",
              COUNT(*) FILTER (WHERE status = 'done')        AS "done!: i64"
            FROM tasks
            WHERE assignee_id = $1
              AND project_id = ANY($2)
              AND deleted_at IS NULL
            "#,
            user.id,
            &accessible
        )
        .fetch_one(&state.db)
        .await?;
        my_status_counts = StatusCounts {
            todo: sc.todo,
            in_progress: sc.in_progress,
            review: sc.review,
            done: sc.done,
        };
    }

    // ── overdue ──
    let overdue_rows = sqlx::query!(
        r#"
        SELECT t.key      AS "key!: String",
               t.title    AS "title!: String",
               t.due_date AS "due_date!: NaiveDate",
               u.handle   AS "assignee_handle?: String"
        FROM   tasks t
        LEFT JOIN users u ON u.id = t.assignee_id
        WHERE  t.assignee_id = $1
          AND  t.deleted_at IS NULL
          AND  t.status <> 'done'
          AND  t.due_date IS NOT NULL
          AND  t.due_date < $2
        ORDER  BY t.due_date ASC
        LIMIT  10
        "#,
        user.id,
        today
    )
    .fetch_all(&state.db)
    .await?;
    let overdue: Vec<DueRow> = overdue_rows
        .into_iter()
        .map(|r| DueRow {
            task_key: r.key,
            title: r.title,
            due_date: r.due_date,
            assignee_handle: r.assignee_handle,
            days_until: (r.due_date - today).num_days(),
        })
        .collect();

    // ── my tasks sample (top 10 by priority then update recency) ──
    let mine = sqlx::query!(
        r#"
        SELECT t.key          AS "key!: String",
               p.key          AS "project_key!: String",
               t.title        AS "title!: String",
               t.status       AS "status!: String",
               t.priority     AS "priority!: String"
        FROM   tasks t
        JOIN   projects p ON p.id = t.project_id
        WHERE  t.assignee_id = $1 AND t.deleted_at IS NULL AND t.status <> 'done'
        ORDER  BY t.priority ASC, t.updated_at DESC
        LIMIT  10
        "#,
        user.id
    )
    .fetch_all(&state.db)
    .await?;
    let my_tasks_sample: Vec<MyTaskSample> = mine
        .into_iter()
        .map(|r| MyTaskSample {
            key: r.key,
            project_key: r.project_key,
            title: r.title,
            status: r.status,
            priority: r.priority,
        })
        .collect();

    // ── watched changed recently (last 7d) ──
    let since = Utc::now() - Duration::days(7);
    let watched = sqlx::query!(
        r#"
        SELECT DISTINCT ON (t.id)
               t.key         AS "task_key!: String",
               t.title       AS "title!: String",
               a.created_at  AS "last_activity_at!: DateTime<Utc>",
               a.kind        AS "last_kind!: String"
        FROM   task_watchers w
        JOIN   tasks t ON t.id = w.task_id AND t.deleted_at IS NULL
        JOIN   task_activity a ON a.task_id = t.id
        WHERE  w.user_id = $1
           AND a.created_at >= $2
           AND a.actor_id <> $1
        ORDER  BY t.id, a.created_at DESC
        LIMIT  15
        "#,
        user.id,
        since
    )
    .fetch_all(&state.db)
    .await?;
    let watched_changed_recently: Vec<WatchedRow> = watched
        .into_iter()
        .map(|r| WatchedRow {
            task_key: r.task_key,
            title: r.title,
            last_activity_at: r.last_activity_at,
            last_kind: r.last_kind,
        })
        .collect();

    // ── time this week (mine) ──
    let time_this_week_minutes: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(duration_minutes), 0)::bigint
        FROM   time_logs
        WHERE  user_id = $1 AND deleted_at IS NULL
          AND  ended_at IS NOT NULL
          AND  started_at >= $2
        "#,
    )
    .bind(user.id)
    .bind(week_start)
    .fetch_one(&state.db)
    .await?;

    // ── running timer ──
    let running = sqlx::query!(
        r#"
        SELECT t.key         AS "task_key!: String",
               tl.started_at AS "started_at!: DateTime<Utc>"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        WHERE  tl.user_id = $1 AND tl.ended_at IS NULL AND tl.deleted_at IS NULL
        LIMIT  1
        "#,
        user.id
    )
    .fetch_optional(&state.db)
    .await?
    .map(|r| RunningTimerRef {
        task_key: r.task_key,
        started_at: r.started_at,
    });

    Ok(Json(MyDashboardDto {
        my_status_counts,
        overdue,
        my_tasks_sample,
        watched_changed_recently,
        time_this_week_minutes,
        running_timer: running,
    }))
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn monday_utc(now: DateTime<Utc>) -> DateTime<Utc> {
    use chrono::{Datelike, NaiveTime, TimeZone};
    let date = now.date_naive();
    let offset = date.weekday().num_days_from_monday() as i64;
    let monday = date - chrono::Duration::days(offset);
    Utc.from_utc_datetime(&chrono::NaiveDateTime::new(monday, NaiveTime::MIN))
}

async fn accessible_project_ids(db: &PgPool, user: &CurrentUser) -> AppResult<Vec<Uuid>> {
    if user.role == GlobalRole::Admin {
        Ok(
            sqlx::query_scalar(r#"SELECT id FROM projects WHERE deleted_at IS NULL"#)
                .fetch_all(db)
                .await?,
        )
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
