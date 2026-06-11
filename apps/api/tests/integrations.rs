//! F1 — Git integration: linking commits/PRs to tasks, provider connections,
//! and outbound status job enqueueing. (Signature verification, key parsing,
//! and status-request shapes are unit-tested in the domain modules.)

use sprintly_api::domain::integrations;
use sqlx::PgPool;
use uuid::Uuid;

const MASTER_KEY: &[u8; 32] = b"0123456789abcdef0123456789abcdef";

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
        Some("abc1234def5678abc1234def5678abc1234def56"),
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
        Some("abc1234def5678abc1234def5678abc1234def56"),
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
    let r = integrations::link(&pool, "NOPE-9", "commit", "abc", None, None, None, None)
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
        Some("feedfacefeedfacefeedfacefeedfacefeedface"),
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
        Some("feedfacefeedfacefeedfacefeedfacefeedface"),
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

    // The head SHA was captured for outbound status.
    let sha: Option<String> = sqlx::query_scalar(
        "SELECT sha FROM git_links WHERE task_id = $1 AND kind = 'pull_request'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        sha.as_deref(),
        Some("feedfacefeedfacefeedfacefeedfacefeedface")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn integration_secrets_round_trip_and_stay_hidden(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();

    let gi = integrations::create_integration(
        &pool,
        MASTER_KEY,
        project_id,
        "github",
        "acme/app",
        None,
        Some("hook-secret"),
        Some("api-token-123"),
        true,
        None,
    )
    .await
    .unwrap();
    assert!(gi.has_webhook_secret);
    assert!(gi.has_api_token);

    // The DTO never leaks plaintext; decryption round-trips.
    let json = serde_json::to_string(&gi).unwrap();
    assert!(!json.contains("api-token-123"));
    assert!(!json.contains("hook-secret"));
    let token = integrations::decrypt_api_token(&pool, MASTER_KEY, gi.id)
        .await
        .unwrap();
    assert_eq!(token.as_deref(), Some("api-token-123"));

    // Same repo twice → conflict.
    assert!(integrations::create_integration(
        &pool, MASTER_KEY, project_id, "github", "acme/app", None, None, None, false, None,
    )
    .await
    .is_err());

    integrations::delete_integration(&pool, gi.id, project_id)
        .await
        .unwrap();
    assert!(integrations::list_integrations(&pool, project_id)
        .await
        .unwrap()
        .is_empty());
    // Token gone with the row.
    assert!(integrations::decrypt_api_token(&pool, MASTER_KEY, gi.id)
        .await
        .unwrap()
        .is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn status_jobs_enqueue_only_when_eligible(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();
    async fn jobs(pool: &PgPool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE kind = 'push_commit_status'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    // No integration yet: linking a commit (with SHA) enqueues nothing.
    integrations::link(
        &pool,
        "GH-1",
        "commit",
        "abc1234",
        None,
        Some("m"),
        None,
        Some("abc1234def5678abc1234def5678abc1234def56"),
    )
    .await
    .unwrap();
    assert_eq!(jobs(&pool).await, 0, "no connection → no job");

    // Status-enabled connection with a token: the next link enqueues.
    integrations::create_integration(
        &pool,
        MASTER_KEY,
        project_id,
        "github",
        "acme/app",
        None,
        None,
        Some("tok"),
        true,
        None,
    )
    .await
    .unwrap();
    integrations::queue_status_updates(&pool, task)
        .await
        .unwrap();
    assert_eq!(jobs(&pool).await, 1, "eligible task → job enqueued");

    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE kind = 'push_commit_status' LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        payload.get("task_id").and_then(|v| v.as_str()),
        Some(task.to_string().as_str())
    );
}
