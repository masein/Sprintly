//! F9 — task templates + recurrence materialisation + bulk ops.

use chrono::{DateTime, TimeZone, Utc};
use sprintly_api::domain::templates;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'T', 'x', 'member', 'active')"#,
    )
    .bind(id)
    .bind(format!("{}@x.test", id.simple()))
    .bind(format!("u{}", id.simple()))
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Project with a default board and one todo column. Returns (pid, board, col).
async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $2, $3)"#)
        .bind(pid)
        .bind(key)
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
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'To do', 'todo', 1024.0)"#,
    )
    .bind(col)
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    (pid, board, col)
}

async fn make_task(pool: &PgPool, pid: Uuid, board: Uuid, col: Uuid, key: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 'todo', 1024.0)"#,
    )
    .bind(id)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(key)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_sprint(pool: &PgPool, pid: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at)
           VALUES ($1, $2, 'S1', now(), now() + interval '14 days')"#,
    )
    .bind(id)
    .bind(pid)
    .execute(pool)
    .await
    .unwrap();
    id
}

fn at(y: i32, mo: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, 9, 0, 0).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn instantiate_prefills_task_fields(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "TP", owner).await;

    let t = templates::create(
        &pool,
        pid,
        "Weekly review",
        "Run the weekly review",
        "the agenda…",
        "chore",
        "p1",
        &["ops".to_string(), "ritual".to_string()],
        "none",
        None,
    )
    .await
    .unwrap();

    let (task_id, key) = templates::instantiate(&pool, &t, Some(owner), None)
        .await
        .unwrap();
    assert!(key.starts_with("TP-"));

    let (title, ty, prio, labels): (String, String, String, Vec<String>) =
        sqlx::query_as("SELECT title, type, priority, labels FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(title, "Run the weekly review");
    assert_eq!(ty, "chore");
    assert_eq!(prio, "p1");
    assert_eq!(labels, vec!["ops".to_string(), "ritual".to_string()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn weekly_template_materialises_each_week(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "RC", owner).await;

    // First run scheduled for Jan 8.
    let t = templates::create(
        &pool,
        pid,
        "Standup notes",
        "Standup notes",
        "",
        "chore",
        "p2",
        &[],
        "weekly",
        Some(at(2026, 1, 8)),
    )
    .await
    .unwrap();
    assert_eq!(t.recurrence, "weekly");

    let task_count = |pool: PgPool| async move {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tasks WHERE project_id = $1")
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    // Before the first run: nothing.
    assert!(templates::materialise_due(&pool, at(2026, 1, 7))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(task_count(pool.clone()).await, 0);

    // On the run date: one task, and next_run advances a week.
    let made = templates::materialise_due(&pool, at(2026, 1, 8))
        .await
        .unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(task_count(pool.clone()).await, 1);
    let next: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT next_run_at FROM task_templates WHERE id = $1")
            .bind(t.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(next, Some(at(2026, 1, 15)), "advanced one week");

    // Re-running the same day doesn't double up.
    assert!(templates::materialise_due(&pool, at(2026, 1, 8))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(task_count(pool.clone()).await, 1);

    // The next week spawns another.
    assert_eq!(
        templates::materialise_due(&pool, at(2026, 1, 15))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(task_count(pool.clone()).await, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn bulk_assign_then_sprint_moves_off_backlog(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project(&pool, "BK", owner).await;
    make_task(&pool, pid, board, col, "BK-1").await;
    make_task(&pool, pid, board, col, "BK-2").await;
    make_task(&pool, pid, board, col, "BK-3").await;

    // All three start in the backlog (no sprint).
    assert_eq!(templates::backlog(&pool, pid).await.unwrap().len(), 3);

    let keys = vec!["BK-1".to_string(), "BK-2".to_string(), "BK-3".to_string()];
    let n = templates::bulk_assign(&pool, pid, &keys, Some(owner))
        .await
        .unwrap();
    assert_eq!(n, 3);
    let assigned: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tasks WHERE project_id = $1 AND assignee_id = $2")
            .bind(pid)
            .bind(owner)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(assigned, 3, "all selected got the assignee");

    // Bulk-move two of them into a sprint → they leave the backlog.
    let sprint = make_sprint(&pool, pid).await;
    let two = vec!["BK-1".to_string(), "BK-2".to_string()];
    assert_eq!(
        templates::bulk_sprint(&pool, pid, &two, Some(sprint))
            .await
            .unwrap(),
        2
    );
    let backlog = templates::backlog(&pool, pid).await.unwrap();
    assert_eq!(backlog.len(), 1, "only BK-3 remains unscheduled");
    assert_eq!(backlog[0].key, "BK-3");

    // Keys outside the project (or absent) are no-ops, not errors.
    assert_eq!(
        templates::bulk_delete(&pool, pid, &["NOPE-9".to_string()])
            .await
            .unwrap(),
        0
    );
}
