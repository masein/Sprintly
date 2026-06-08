//! F1 — Git integration: linking commits/PRs to tasks. (Signature verification
//! and key parsing are unit-tested in the domain module.)

use sprintly_api::domain::integrations;
use sqlx::PgPool;
use uuid::Uuid;

/// Create a task with a known key and return its id.
async fn make_task(pool: &PgPool, key: &str) -> Uuid {
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
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'GH', 'GitHub', $2)"#,
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
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'Todo', 'todo', 1024.0)"#,
    )
    .bind(col)
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    // A done column so merge auto-transition has somewhere to land.
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'Done', 'done', 4096.0)"#,
    )
    .bind(Uuid::now_v7())
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    let task = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 'todo', 1024.0)"#,
    )
    .bind(task)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(key)
    .execute(pool)
    .await
    .unwrap();
    task
}

async fn count(pool: &PgPool, sql: &str, task: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(task)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn link_creates_then_dedupes(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;

    let first = integrations::link(
        &pool,
        "GH-1",
        "commit",
        "abc1234",
        Some("http://x/c"),
        Some("msg"),
        None,
    )
    .await
    .unwrap();
    assert!(first, "first link is created");

    let second = integrations::link(
        &pool,
        "GH-1",
        "commit",
        "abc1234",
        Some("http://x/c"),
        Some("msg"),
        None,
    )
    .await
    .unwrap();
    assert!(!second, "same ref de-duplicates");

    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM git_links WHERE task_id = $1",
            task
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM task_activity WHERE task_id = $1 AND kind = 'commit_linked'",
            task
        )
        .await,
        1,
        "activity logged exactly once"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_key_is_noop(pool: PgPool) {
    make_task(&pool, "GH-1").await;
    let r = integrations::link(&pool, "NOPE-9", "commit", "abc", None, None, None)
        .await
        .unwrap();
    assert!(!r);
}

#[sqlx::test(migrations = "./migrations")]
async fn pr_merge_updates_state_and_logs(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    integrations::link(
        &pool,
        "GH-1",
        "pull_request",
        "#5",
        Some("http://x/pr/5"),
        Some("My PR"),
        Some("open"),
    )
    .await
    .unwrap();
    integrations::link(
        &pool,
        "GH-1",
        "pull_request",
        "#5",
        Some("http://x/pr/5"),
        Some("My PR"),
        Some("merged"),
    )
    .await
    .unwrap();

    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM task_activity WHERE task_id = $1 AND kind = 'pr_merged'",
            task
        )
        .await,
        1
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM git_links WHERE task_id = $1 AND kind = 'pull_request'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, "merged");

    // The merge auto-transitioned the task into the done column.
    let (status, category): (String, String) = sqlx::query_as(
        "SELECT t.status, bc.category FROM tasks t
         JOIN board_columns bc ON bc.id = t.column_id WHERE t.id = $1",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "done");
    assert_eq!(category, "done");
}
