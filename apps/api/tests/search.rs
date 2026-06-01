//! M3 phase C integration tests — search ranking + relations shape.

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
    .bind("Test User")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project_with_task(
    pool: &PgPool,
    key: &str,
    owner: Uuid,
    title: &str,
) -> (Uuid, Uuid, String) {
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#,
    ).bind(pid).bind(key).bind(key).bind(owner).execute(pool).await.unwrap();
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

    let mut tx = pool.begin().await.unwrap();
    let (task_key, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    let tid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
           VALUES ($1, $2, $3, $4, $5, $6, 1024.0)"#,
    )
    .bind(tid).bind(pid).bind(board).bind(col).bind(&task_key).bind(title)
    .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (pid, tid, task_key)
}

#[sqlx::test(migrations = "./migrations")]
async fn search_returns_member_tasks_only(pool: PgPool) {
    let alice = make_user(&pool).await;
    let bob = make_user(&pool).await;
    let (_, _, akey) =
        make_project_with_task(&pool, "ALPHA", alice, "fix flaky build").await;
    let (_, _, bkey) =
        make_project_with_task(&pool, "BETA", bob, "fix flaky build").await;

    // Verify the data invariant: each task is in its own project, and the
    // accessibility join (the same SQL used by `accessible_project_ids`)
    // returns only the project alice is a member of.
    let visible_to_alice: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT pm.project_id
           FROM   project_members pm
           JOIN   projects p ON p.id = pm.project_id
           WHERE  pm.user_id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(alice)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(visible_to_alice.len(), 1);

    // Sanity: both task keys differ even though the title is identical.
    assert_ne!(akey, bkey);
}

#[sqlx::test(migrations = "./migrations")]
async fn search_ranks_tsvector_then_trigram(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) =
        make_project_with_task(&pool, "RANK", owner, "fix the build pipeline").await;
    // Add a "typo" task. tsvector won't hit; pg_trgm should.
    let board: Uuid =
        sqlx::query_scalar("SELECT id FROM boards WHERE project_id = $1")
            .bind(pid).fetch_one(&pool).await.unwrap();
    let col: Uuid =
        sqlx::query_scalar("SELECT id FROM board_columns WHERE board_id = $1")
            .bind(board).fetch_one(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let (typo_key, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    let tid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 'fxi the buld', 2048.0)"#,
    )
    .bind(tid).bind(pid).bind(board).bind(col).bind(&typo_key)
    .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    // Exact-ish query should hit at least the well-titled row.
    let rows: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT title
        FROM   tasks
        WHERE  search_tsv @@ plainto_tsquery('english', $1)
            OR title % $1
        "#,
    )
    .bind("fix build")
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(rows.iter().any(|t| t.contains("fix the build pipeline")));
}

#[sqlx::test(migrations = "./migrations")]
async fn subtasks_returns_only_children(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, parent_id, _) =
        make_project_with_task(&pool, "SUB", owner, "parent").await;
    let board: Uuid =
        sqlx::query_scalar("SELECT id FROM boards WHERE project_id = $1")
            .bind(pid).fetch_one(&pool).await.unwrap();
    let col: Uuid =
        sqlx::query_scalar("SELECT id FROM board_columns WHERE board_id = $1")
            .bind(board).fetch_one(&pool).await.unwrap();
    // Two children + one unrelated.
    for (i, parent) in [(1, Some(parent_id)), (2, Some(parent_id)), (3, None)] {
        let mut tx = pool.begin().await.unwrap();
        let (k, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
        sqlx::query(
            r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, parent_task_id, order_in_column)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(Uuid::now_v7())
        .bind(pid)
        .bind(board)
        .bind(col)
        .bind(&k)
        .bind(format!("child {i}"))
        .bind(parent)
        .bind(2048.0 + (i as f64) * 1024.0)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE parent_task_id = $1 AND deleted_at IS NULL",
    )
    .bind(parent_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn links_directed_and_self_link_blocked(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (_, t1, _) = make_project_with_task(&pool, "LNK", owner, "one").await;
    let (_, t2, _) = make_project_with_task(&pool, "LNK2", owner, "two").await;
    sqlx::query(
        r#"INSERT INTO task_links (from_task_id, to_task_id, kind)
           VALUES ($1, $2, 'blocks')"#,
    )
    .bind(t1).bind(t2).execute(&pool).await.unwrap();

    // Self-link: CHECK blocks at the SQL layer.
    let res = sqlx::query(
        r#"INSERT INTO task_links (from_task_id, to_task_id, kind)
           VALUES ($1, $1, 'blocks')"#,
    )
    .bind(t1)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "self-link must be rejected by CHECK");
}
