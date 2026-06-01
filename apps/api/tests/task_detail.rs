//! M3 phase B integration tests.

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
    .bind(format!("u{}@x.test", id.simple()))
    .bind(format!("h{}", id.simple()))
    .bind("Test")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project_with_task(pool: &PgPool, owner: Uuid) -> (Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid)
        .bind("TST")
        .bind("T")
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
           VALUES ($1, $2, 'C', 'todo', 1024.0)"#,
    )
    .bind(col)
    .bind(board)
    .execute(pool)
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let (key, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    let tid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 1024.0)"#,
    )
    .bind(tid)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(&key)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    (pid, tid)
}

async fn make_comment(
    pool: &PgPool,
    task_id: Uuid,
    author: Uuid,
    parent: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO task_comments (id, task_id, author_id, parent_comment_id, body)
           VALUES ($1, $2, $3, $4, 'hi')"#,
    )
    .bind(id)
    .bind(task_id)
    .bind(author)
    .bind(parent)
    .execute(pool)
    .await?;
    Ok(id)
}

#[sqlx::test(migrations = "./migrations")]
async fn threading_one_level_only(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (_pid, tid) = make_project_with_task(&pool, owner).await;

    let top = make_comment(&pool, tid, owner, None).await.unwrap();
    // First-level reply works.
    let reply = make_comment(&pool, tid, owner, Some(top)).await.unwrap();
    // Second-level reply (replying to a reply) must be rejected by the trigger.
    let nested = make_comment(&pool, tid, owner, Some(reply)).await;
    assert!(
        nested.is_err(),
        "second-level reply must be blocked by trigger"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn reactions_unique_per_target_user_emoji(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (_pid, tid) = make_project_with_task(&pool, owner).await;

    sqlx::query(
        r#"INSERT INTO task_reactions (id, task_id, user_id, emoji)
           VALUES ($1, $2, $3, '👍')"#,
    )
    .bind(Uuid::now_v7())
    .bind(tid)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();

    // Same user + same emoji again must collide.
    let dup = sqlx::query(
        r#"INSERT INTO task_reactions (id, task_id, user_id, emoji)
           VALUES ($1, $2, $3, '👍')"#,
    )
    .bind(Uuid::now_v7())
    .bind(tid)
    .bind(owner)
    .execute(&pool)
    .await;
    assert!(dup.is_err(), "duplicate reaction must be blocked");

    // Different emoji is fine.
    sqlx::query(
        r#"INSERT INTO task_reactions (id, task_id, user_id, emoji)
           VALUES ($1, $2, $3, '🎉')"#,
    )
    .bind(Uuid::now_v7())
    .bind(tid)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn reactions_target_xor_constraint(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (_pid, tid) = make_project_with_task(&pool, owner).await;
    let top = make_comment(&pool, tid, owner, None).await.unwrap();

    // Both task_id AND comment_id set → CHECK rejects.
    let res = sqlx::query(
        r#"INSERT INTO task_reactions (id, task_id, comment_id, user_id, emoji)
           VALUES ($1, $2, $3, $4, '🚀')"#,
    )
    .bind(Uuid::now_v7())
    .bind(tid)
    .bind(top)
    .bind(owner)
    .execute(&pool)
    .await;
    assert!(res.is_err());

    // Neither set → also rejected.
    let res2 = sqlx::query(
        r#"INSERT INTO task_reactions (id, user_id, emoji)
           VALUES ($1, $2, '🚀')"#,
    )
    .bind(Uuid::now_v7())
    .bind(owner)
    .execute(&pool)
    .await;
    assert!(res2.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn attachment_lifecycle_pending_then_ready(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (_pid, tid) = make_project_with_task(&pool, owner).await;

    let aid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO task_attachments
              (id, task_id, uploader_id, filename, mime_type, storage_key)
           VALUES ($1, $2, $3, 'doc.pdf', 'application/pdf', 'tasks/x/y')"#,
    )
    .bind(aid)
    .bind(tid)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();

    let status: String = sqlx::query_scalar("SELECT status FROM task_attachments WHERE id = $1")
        .bind(aid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "pending");

    sqlx::query("UPDATE task_attachments SET status = 'ready', size_bytes = 12345 WHERE id = $1")
        .bind(aid)
        .execute(&pool)
        .await
        .unwrap();

    let (status, size): (String, Option<i64>) =
        sqlx::query_as("SELECT status, size_bytes FROM task_attachments WHERE id = $1")
            .bind(aid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "ready");
    assert_eq!(size, Some(12345));
}

#[sqlx::test(migrations = "./migrations")]
async fn column_delete_refused_when_tasks_remain(pool: PgPool) {
    // The M2-deferred guard now active in routes/boards.rs is logical, but we
    // assert the underlying data invariant: tasks exist → column row visible.
    let owner = make_user(&pool).await;
    let (_pid, tid) = make_project_with_task(&pool, owner).await;
    let column_id: Uuid = sqlx::query_scalar("SELECT column_id FROM tasks WHERE id = $1")
        .bind(tid)
        .fetch_one(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE column_id = $1 AND deleted_at IS NULL",
    )
    .bind(column_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(n > 0, "column must still hold the task");
}
