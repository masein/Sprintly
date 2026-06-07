//! F2 — webhook delivery: dispatch enqueues jobs for the right subscriptions.
//! (The signed POST itself is exercised by the functional/e2e checks.)

use serde_json::json;
use sprintly_api::domain::webhooks;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_project(pool: &PgPool) -> Uuid {
    let owner = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'T', 'x', 'member', 'active')"#,
    )
    .bind(owner)
    .bind(format!("{}@x.test", owner.simple()))
    .bind(format!("h{}", &owner.simple().to_string()[..10]))
    .execute(pool)
    .await
    .unwrap();
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'WHK', 'Hooks', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    pid
}

async fn add_hook(pool: &PgPool, pid: Uuid, events: &[&str], active: bool, configured: bool) {
    let ev: Vec<String> = events.iter().map(|s| s.to_string()).collect();
    sqlx::query(
        r#"INSERT INTO webhooks (id, project_id, url, secret_ciphertext, secret_nonce, events, active)
           VALUES ($1, $2, 'https://example.test/hook', $3, $4, $5, $6)"#,
    )
    .bind(Uuid::now_v7())
    .bind(pid)
    .bind(if configured { Some(vec![1u8; 16]) } else { None })
    .bind(if configured { Some(vec![0u8; 24]) } else { None })
    .bind(&ev)
    .bind(active)
    .execute(pool)
    .await
    .unwrap();
}

async fn deliver_jobs(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'deliver_webhook'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn dispatch_enqueues_only_for_matching_subscribers(pool: PgPool) {
    let pid = make_project(&pool).await;
    add_hook(&pool, pid, &["task.created"], true, true).await; // matches
    add_hook(&pool, pid, &["comment.created"], true, true).await; // wrong event
    add_hook(&pool, pid, &["task.created"], false, true).await; // inactive
    add_hook(&pool, pid, &["task.created"], true, false).await; // unconfigured (no secret)

    let n = webhooks::dispatch(&pool, pid, "task.created", json!({ "key": "WHK-1" }))
        .await
        .unwrap();

    assert_eq!(n, 1, "only the active, configured, matching webhook fires");
    assert_eq!(deliver_jobs(&pool).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn dispatch_no_subscribers_is_noop(pool: PgPool) {
    let pid = make_project(&pool).await;
    let n = webhooks::dispatch(&pool, pid, "task.created", json!({}))
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(deliver_jobs(&pool).await, 0);
}
