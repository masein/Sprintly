//! F13 — flow metrics computation over real task rows.

use chrono::{Duration, Utc};
use sprintly_api::domain::metrics;
use sqlx::PgPool;
use uuid::Uuid;

/// Returns (project_id, board_id, todo_col, done_col).
async fn setup(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let owner = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'T', 'x', 'member', 'active')"#,
    )
    .bind(owner)
    .bind(format!("{}@x.test", owner.simple()))
    .bind(format!("u{}", &owner.simple().to_string()[..10]))
    .execute(pool)
    .await
    .unwrap();
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'MET', 'Metrics', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    let board = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    )
    .bind(board)
    .bind(pid)
    .execute(pool)
    .await
    .unwrap();
    let todo = Uuid::now_v7();
    let done = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO board_columns (id, board_id, name, category, sort_order) VALUES ($1,$2,'Todo','todo',1.0),($3,$4,'Done','done',2.0)"#)
        .bind(todo).bind(board).bind(done).bind(board)
        .execute(pool)
        .await
        .unwrap();
    (pid, board, todo, done)
}

#[allow(clippy::too_many_arguments)]
async fn task(
    pool: &PgPool,
    pid: Uuid,
    board: Uuid,
    col: Uuid,
    key: &str,
    status: &str,
    created_days_ago: i64,
    completed_days_ago: Option<i64>,
) {
    let created = Utc::now() - Duration::days(created_days_ago);
    let completed = completed_days_ago.map(|d| Utc::now() - Duration::days(d));
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, order_in_column, created_at, completed_at)
           VALUES ($1, $2, $3, $4, $5, 't', $6, 1024.0, $7, $8)"#,
    )
    .bind(Uuid::now_v7())
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(key)
    .bind(status)
    .bind(created)
    .bind(completed)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn computes_lead_throughput_and_wip(pool: PgPool) {
    let (pid, board, todo, done) = setup(&pool).await;
    task(&pool, pid, board, done, "MET-1", "done", 10, Some(8)).await; // 48h lead
    task(&pool, pid, board, done, "MET-2", "done", 5, Some(4)).await; // 24h lead
    task(&pool, pid, board, todo, "MET-3", "in_progress", 3, None).await;
    task(&pool, pid, board, todo, "MET-4", "todo", 2, None).await;

    let m = metrics::compute(&pool, pid, 8).await.unwrap();

    assert_eq!(m.lead_time.count, 2);
    assert!(
        (m.lead_time.avg_hours - 36.0).abs() < 1.0,
        "avg lead ~36h, got {}",
        m.lead_time.avg_hours
    );
    assert!(m.lead_time.p90_hours >= m.lead_time.p50_hours);

    let total: i64 = m.throughput.iter().map(|p| p.count).sum();
    assert_eq!(total, 2, "two tasks completed in the window");

    assert_eq!(m.wip.in_progress, 1);
    assert_eq!(m.wip.todo, 1);
    assert_eq!(m.wip.review, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn old_completions_fall_outside_window(pool: PgPool) {
    let (pid, board, _todo, done) = setup(&pool).await;
    task(&pool, pid, board, done, "MET-9", "done", 100, Some(90)).await; // 90d ago

    let m = metrics::compute(&pool, pid, 8).await.unwrap(); // 8-week window
    assert_eq!(m.lead_time.count, 0);
    assert_eq!(m.throughput.iter().map(|p| p.count).sum::<i64>(), 0);
}
