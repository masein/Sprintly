//! M9 integration tests — achievement awarding + idempotency.

use sprintly_api::{
    config::AuthConfig,
    domain::{
        achievements::{award_batch, scan_all},
        password,
        tasks as task_domain,
    },
};
use serde_json::json;
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

async fn make_project_with_task_done(pool: &PgPool, owner: Uuid, n: usize) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'A', 'A', $2)"#)
        .bind(pid).bind(owner).execute(pool).await.unwrap();
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
           VALUES ($1, $2, 'D', 'done', 1024.0)"#,
    ).bind(col).bind(board).execute(pool).await.unwrap();

    for _ in 0..n {
        let mut tx = pool.begin().await.unwrap();
        let (k, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
        sqlx::query(
            r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status,
                                  assignee_id, order_in_column)
               VALUES ($1, $2, $3, $4, $5, 't', 'done', $6, 1024.0)"#,
        )
        .bind(Uuid::now_v7()).bind(pid).bind(board).bind(col).bind(&k).bind(owner)
        .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn catalog_was_seeded(pool: PgPool) {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM achievements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(n >= 8, "catalog should have at least 8 seeded rows (got {n})");

    let codes: Vec<String> = sqlx::query_scalar("SELECT code FROM achievements ORDER BY code")
        .fetch_all(&pool)
        .await
        .unwrap();
    for needed in [
        "BUG_SLAYER", "COFFEE_ADDICT", "ESTIMATOR_SUPREME", "PR_WIZARD",
        "RETRO_HERO", "RTFM", "SPRINT_CLOSER", "WATCHER_IN_WHEAT_FIELD",
    ] {
        assert!(codes.iter().any(|c| c == needed), "missing {needed}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn pk_dedupes_repeated_awards(pool: PgPool) {
    let u = make_user(&pool).await;
    let aid: Uuid = sqlx::query_scalar("SELECT id FROM achievements WHERE code = 'RTFM'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let first = sqlx::query(
        r#"INSERT INTO user_achievements (user_id, achievement_id) VALUES ($1, $2)
           ON CONFLICT DO NOTHING"#,
    )
    .bind(u).bind(aid).execute(&pool).await.unwrap();
    assert_eq!(first.rows_affected(), 1);

    let again = sqlx::query(
        r#"INSERT INTO user_achievements (user_id, achievement_id) VALUES ($1, $2)
           ON CONFLICT DO NOTHING"#,
    )
    .bind(u).bind(aid).execute(&pool).await.unwrap();
    assert_eq!(again.rows_affected(), 0, "PK must collapse re-runs");
}

#[sqlx::test(migrations = "./migrations")]
async fn pr_wizard_returns_users_with_50_done(pool: PgPool) {
    let u = make_user(&pool).await;
    // Below the threshold.
    make_project_with_task_done(&pool, u, 49).await;
    let batches = scan_all(&pool).await.unwrap();
    let pr = batches.iter().find(|(c, _)| *c == "PR_WIZARD").unwrap();
    assert!(pr.1.is_empty(), "49 done tasks shouldn't trigger PR_WIZARD");

    // Top up to 50.
    make_project_with_task_done(&pool, u, 1).await;
    let batches = scan_all(&pool).await.unwrap();
    let pr = batches.iter().find(|(c, _)| *c == "PR_WIZARD").unwrap();
    assert_eq!(pr.1.len(), 1, "50 done tasks should trigger PR_WIZARD");
    assert_eq!(pr.1[0].0, u);
}

#[sqlx::test(migrations = "./migrations")]
async fn award_batch_is_idempotent(pool: PgPool) {
    let u = make_user(&pool).await;
    let candidates = vec![(u, json!({ "count": 50 }))];
    let n1 = award_batch(&pool, "PR_WIZARD", &candidates).await.unwrap();
    let n2 = award_batch(&pool, "PR_WIZARD", &candidates).await.unwrap();
    assert_eq!(n1, 1, "first award inserts");
    assert_eq!(n2, 0, "second award is a no-op (ON CONFLICT)");
}

#[sqlx::test(migrations = "./migrations")]
async fn award_batch_returns_zero_for_unknown_code(pool: PgPool) {
    let u = make_user(&pool).await;
    let n = award_batch(&pool, "NOT_A_REAL_CODE", &[(u, json!({}))])
        .await
        .unwrap();
    assert_eq!(n, 0);
}
