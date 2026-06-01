//! M3 phase A integration tests.

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
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, $4, $5, 'member')
        "#,
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

async fn make_project_with_board(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#,
    )
    .bind(pid).bind(key).bind(key).bind(owner)
    .execute(pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, 'lead', $2)"#,
    ).bind(pid).bind(owner).execute(pool).await.unwrap();

    let board_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    ).bind(board_id).bind(pid).execute(pool).await.unwrap();

    let col_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'To do', 'todo', 1024.0)"#,
    ).bind(col_id).bind(board_id).execute(pool).await.unwrap();
    (pid, board_id, col_id)
}

async fn make_task(
    pool: &PgPool,
    project_id: Uuid,
    board_id: Uuid,
    column_id: Uuid,
    order_in_column: f64,
) -> (Uuid, String) {
    let mut tx = pool.begin().await.unwrap();
    let (key, _) = task_domain::next_key(&mut tx, project_id).await.unwrap();
    let task_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(task_id)
    .bind(project_id)
    .bind(board_id)
    .bind(column_id)
    .bind(&key)
    .bind("Test")
    .bind(order_in_column)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (task_id, key)
}

#[sqlx::test(migrations = "./migrations")]
async fn task_keys_increment_per_project(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (p1, _, _) = make_project_with_board(&pool, "ALPHA", owner).await;
    let (p2, _, _) = make_project_with_board(&pool, "BETA", owner).await;

    let mut tx = pool.begin().await.unwrap();
    let (a1, _) = task_domain::next_key(&mut tx, p1).await.unwrap();
    let (a2, _) = task_domain::next_key(&mut tx, p1).await.unwrap();
    let (b1, _) = task_domain::next_key(&mut tx, p2).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(a1, "ALPHA-1");
    assert_eq!(a2, "ALPHA-2");
    assert_eq!(b1, "BETA-1", "second project resets the sequence");
}

#[sqlx::test(migrations = "./migrations")]
async fn task_keys_unique_per_project(pool: PgPool) {
    // The (project_id, key) composite uniqueness must hold even if someone
    // forces a manual insert. The unique partial index is the contract here.
    let owner = make_user(&pool).await;
    let (p1, board, col) = make_project_with_board(&pool, "DUP", owner).await;
    make_task(&pool, p1, board, col, 1024.0).await;

    // Manually insert with a colliding key.
    let res = sqlx::query(
        r#"
        INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
        VALUES ($1, $2, $3, $4, 'DUP-1', 'collides', 2048.0)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(p1)
    .bind(board)
    .bind(col)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "duplicate (project, key) must be rejected");
}

#[sqlx::test(migrations = "./migrations")]
async fn resolve_position_between_two_tasks(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "POS", owner).await;
    let (t1, _) = make_task(&pool, pid, board, col, 1024.0).await;
    let (t2, _) = make_task(&pool, pid, board, col, 2048.0).await;

    let between = task_domain::resolve_position(&pool, col, Some(t1), None).await.unwrap();
    assert!(
        between > 1024.0 && between < 2048.0,
        "drop-after-t1 must land strictly between t1 and t2 (got {between})"
    );

    let before_t2 = task_domain::resolve_position(&pool, col, None, Some(t2)).await.unwrap();
    assert!(
        before_t2 > 1024.0 && before_t2 < 2048.0,
        "drop-before-t2 must also land between t1 and t2 (got {before_t2})"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn resolve_position_append_when_no_hints(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "APP", owner).await;
    let (_, _) = make_task(&pool, pid, board, col, 1024.0).await;
    let (_, _) = make_task(&pool, pid, board, col, 2048.0).await;

    let appended = task_domain::resolve_position(&pool, col, None, None).await.unwrap();
    assert!(appended > 2048.0);
}
