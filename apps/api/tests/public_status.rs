//! Integration tests for public status pages (F18): enabling exposes a
//! whitelisted summary at a token; disabling 404s; sprint progress is computed
//! from real tasks.

use chrono::{Duration, Utc};
use sprintly_api::{config::AuthConfig, domain::password, domain::public_status as ps};
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

/// Project + default board + one "To do" column. Returns (project_id, column_id).
async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid)
        .bind(key)
        .bind(format!("{key} Project"))
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
    (pid, col)
}

async fn add_task(
    pool: &PgPool,
    pid: Uuid,
    col: Uuid,
    board: Uuid,
    sprint: Option<Uuid>,
    status: &str,
    n: i32,
) {
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, sprint_id, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 'task', $6, $7, $8)"#,
    )
    .bind(Uuid::now_v7())
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(format!("T-{n}"))
    .bind(status)
    .bind(sprint)
    .bind(n as f64 * 1024.0)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn enable_exposes_summary_and_disable_invalidates(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, col) = make_project(&pool, "PUB", owner).await;
    let board: Uuid = sqlx::query_scalar("SELECT id FROM boards WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

    // An active sprint with 3 tasks, 2 done.
    let sprint = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, state, starts_at, ends_at)
           VALUES ($1, $2, 'Sprint 1', 'active', $3, $4)"#,
    )
    .bind(sprint)
    .bind(pid)
    .bind(Utc::now() - Duration::days(2))
    .bind(Utc::now() + Duration::days(5))
    .execute(&pool)
    .await
    .unwrap();
    add_task(&pool, pid, col, board, Some(sprint), "done", 1).await;
    add_task(&pool, pid, col, board, Some(sprint), "done", 2).await;
    add_task(&pool, pid, col, board, Some(sprint), "todo", 3).await;

    // Disabled → no token, public view 404s.
    assert!(ps::current_token(&pool, pid).await.unwrap().is_none());

    let token = ps::enable(&pool, pid).await.unwrap();
    // Enabling again is stable (same URL).
    assert_eq!(ps::enable(&pool, pid).await.unwrap(), token);

    let view = ps::load_by_token(&pool, &token).await.unwrap();
    assert_eq!(view.project_name, "PUB Project");
    assert_eq!(view.project_key, "PUB");
    let sp = view.sprint.expect("active sprint");
    assert_eq!(sp.total, 3);
    assert_eq!(sp.done, 2);
    assert_eq!(sp.percent, 66);
    // The whitelisted column summary carries counts, never task content.
    assert_eq!(view.columns.len(), 1);
    assert_eq!(view.columns[0].name, "To do");
    assert_eq!(view.columns[0].count, 3);

    // Disabling invalidates the token.
    ps::disable(&pool, pid).await.unwrap();
    assert!(ps::load_by_token(&pool, &token).await.is_err());
    assert!(ps::current_token(&pool, pid).await.unwrap().is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_token_is_not_found(pool: PgPool) {
    assert!(ps::load_by_token(&pool, "nope-not-a-real-token")
        .await
        .is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn no_active_sprint_yields_none(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _col) = make_project(&pool, "NOS", owner).await;
    let token = ps::enable(&pool, pid).await.unwrap();
    let view = ps::load_by_token(&pool, &token).await.unwrap();
    assert!(view.sprint.is_none());
    assert_eq!(view.columns.len(), 1);
}
