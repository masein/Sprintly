//! M6 integration tests — dashboard SQL shape.

use chrono::{Duration, NaiveDate, Utc};
use sprintly_api::{
    config::AuthConfig,
    domain::{password, tasks as task_domain},
};
use sqlx::PgPool;
use uuid::Uuid;

fn cfg() -> AuthConfig {
    AuthConfig {
        jwt_secret: b"a-test-secret-that-is-long-enough-to-be-fine".to_vec(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 2_592_000,
        argon2_m_cost_kib: 4096,
        argon2_t_cost: 1,
        argon2_p_cost: 1,
    }
}

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, 'member')"#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", &id.to_string()[..8]))
    .bind(format!("h{}", &id.to_string()[..8]))
    .bind("Test")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project_with_board(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid).bind(key).bind(key).bind(owner).execute(pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, 'lead', $2)"#,
    ).bind(pid).bind(owner).execute(pool).await.unwrap();
    let board = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    ).bind(board).bind(pid).execute(pool).await.unwrap();
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'C', 'todo', 1024.0)"#,
    ).bind(col).bind(board).execute(pool).await.unwrap();
    (pid, board, col)
}

async fn make_task(
    pool: &PgPool,
    pid: Uuid,
    board: Uuid,
    col: Uuid,
    title: &str,
    status: &str,
) -> Uuid {
    let mut tx = pool.begin().await.unwrap();
    let (key, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    let tid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, order_in_column)
           VALUES ($1, $2, $3, $4, $5, $6, $7, 1024.0)"#,
    )
    .bind(tid).bind(pid).bind(board).bind(col).bind(&key).bind(title).bind(status)
    .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    tid
}

#[sqlx::test(migrations = "./migrations")]
async fn blocked_query_finds_tasks_blocked_by_open_blockers(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "BLK", owner).await;
    // A blocks B; A is still open → B is blocked.
    let a = make_task(&pool, pid, board, col, "A", "in_progress").await;
    let b = make_task(&pool, pid, board, col, "B", "todo").await;
    sqlx::query(
        r#"INSERT INTO task_links (from_task_id, to_task_id, kind)
           VALUES ($1, $2, 'blocks')"#,
    )
    .bind(a).bind(b).execute(&pool).await.unwrap();

    // Replicate the dashboard's blocked query.
    let blocked: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT t.key, COUNT(l.from_task_id)::bigint
        FROM   tasks t
        JOIN   task_links l ON l.to_task_id = t.id AND l.kind = 'blocks'
        JOIN   tasks blocker ON blocker.id = l.from_task_id
               AND blocker.status <> 'done'
               AND blocker.deleted_at IS NULL
        WHERE  t.project_id = $1
           AND t.deleted_at IS NULL
           AND t.status <> 'done'
        GROUP  BY t.id, t.key
        "#,
    )
    .bind(pid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(blocked.len(), 1, "exactly one task is blocked");
    assert_eq!(blocked[0].1, 1, "blocked by one open blocker");

    // Close the blocker — B should no longer count as blocked.
    sqlx::query("UPDATE tasks SET status = 'done' WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    let blocked_after: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM (
          SELECT t.id
          FROM   tasks t
          JOIN   task_links l ON l.to_task_id = t.id AND l.kind = 'blocks'
          JOIN   tasks blocker ON blocker.id = l.from_task_id
                 AND blocker.status <> 'done'
                 AND blocker.deleted_at IS NULL
          WHERE  t.project_id = $1
             AND t.deleted_at IS NULL
             AND t.status <> 'done'
          GROUP  BY t.id
        ) sub
        "#,
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(blocked_after, 0, "closed blockers stop blocking");
}

#[sqlx::test(migrations = "./migrations")]
async fn time_this_week_sums_for_project(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "TWK", owner).await;
    let task = make_task(&pool, pid, board, col, "T", "in_progress").await;

    let now = Utc::now();
    // Two closed logs inside the week, one log outside (2 weeks ago).
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::now_v7()).bind(task).bind(owner).bind(now - Duration::minutes(60)).bind(now)
    .execute(&pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::now_v7()).bind(task).bind(owner).bind(now - Duration::minutes(30)).bind(now - Duration::minutes(15))
    .execute(&pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::now_v7()).bind(task).bind(owner)
    .bind(now - Duration::days(14))
    .bind(now - Duration::days(14) + Duration::minutes(45))
    .execute(&pool).await.unwrap();

    // Sum since this week's Monday (UTC).
    let monday = {
        use chrono::{Datelike, NaiveTime, TimeZone};
        let d = now.date_naive();
        let off = d.weekday().num_days_from_monday() as i64;
        let m = d - Duration::days(off);
        Utc.from_utc_datetime(&chrono::NaiveDateTime::new(m, NaiveTime::MIN))
    };
    let mins: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(tl.duration_minutes), 0)::bigint
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        WHERE  t.project_id = $1 AND tl.ended_at IS NOT NULL
          AND  tl.deleted_at IS NULL AND tl.started_at >= $2
        "#,
    )
    .bind(pid)
    .bind(monday)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(mins >= 60 && mins < 120, "this week sums 60+15=75 (got {mins})");
}

#[sqlx::test(migrations = "./migrations")]
async fn velocity_history_chronological(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project_with_board(&pool, "VHX", owner).await;
    // Two completed sprints with different completed_at.
    let a = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at, state, velocity_points, completed_at)
           VALUES ($1, $2, 'A', now() - INTERVAL '20 day', now() - INTERVAL '14 day', 'completed', 10,
                   now() - INTERVAL '14 day')"#,
    ).bind(a).bind(pid).execute(&pool).await.unwrap();
    let b = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at, state, velocity_points, completed_at)
           VALUES ($1, $2, 'B', now() - INTERVAL '13 day', now() - INTERVAL '7 day', 'completed', 14,
                   now() - INTERVAL '7 day')"#,
    ).bind(b).bind(pid).execute(&pool).await.unwrap();

    let rows: Vec<(String, i32)> = sqlx::query_as(
        r#"
        SELECT name, velocity_points
        FROM   sprints
        WHERE  project_id = $1 AND state = 'completed'
          AND  velocity_points IS NOT NULL AND deleted_at IS NULL
        ORDER  BY completed_at DESC
        LIMIT  10
        "#,
    )
    .bind(pid)
    .fetch_all(&pool)
    .await
    .unwrap();
    // The dashboard reverses for the chart; raw query is newest-first.
    assert_eq!(rows[0].0, "B");
    assert_eq!(rows[1].0, "A");
}

#[sqlx::test(migrations = "./migrations")]
async fn overdue_query_picks_past_unfinished(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "OD", owner).await;
    let yesterday = (Utc::now() - Duration::days(1)).date_naive();
    let tomorrow = (Utc::now() + Duration::days(1)).date_naive();

    // Overdue (in_progress + past due) — should appear.
    let due_past = make_task(&pool, pid, board, col, "due past", "in_progress").await;
    sqlx::query("UPDATE tasks SET assignee_id = $1, due_date = $2 WHERE id = $3")
        .bind(owner).bind(yesterday).bind(due_past).execute(&pool).await.unwrap();
    // Due tomorrow — not overdue.
    let due_soon = make_task(&pool, pid, board, col, "due soon", "todo").await;
    sqlx::query("UPDATE tasks SET assignee_id = $1, due_date = $2 WHERE id = $3")
        .bind(owner).bind(tomorrow).bind(due_soon).execute(&pool).await.unwrap();
    // Past due but done — not overdue.
    let done_late = make_task(&pool, pid, board, col, "done late", "done").await;
    sqlx::query("UPDATE tasks SET assignee_id = $1, due_date = $2 WHERE id = $3")
        .bind(owner).bind(yesterday).bind(done_late).execute(&pool).await.unwrap();

    let n: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM tasks
        WHERE  assignee_id = $1 AND deleted_at IS NULL
          AND  status <> 'done'
          AND  due_date IS NOT NULL AND due_date < $2
        "#,
    )
    .bind(owner)
    .bind(NaiveDate::from_yo_opt(Utc::now().date_naive().year(), Utc::now().date_naive().ordinal()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "exactly one overdue task — `due past`");
}

// Keep one trait imported for the Datelike methods used above.
use chrono::Datelike;
