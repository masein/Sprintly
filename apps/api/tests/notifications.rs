//! F5 — notifications integration (DB-facing pieces; the live WS push and the
//! HTTP fan-out are exercised by the functional/e2e checks).

use sprintly_api::domain::notifications;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool, handle: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'Test', 'x', 'member', 'active')"#,
    )
    .bind(id)
    .bind(format!("{}@x.test", id.simple()))
    .bind(handle)
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test(migrations = "./migrations")]
async fn resolve_handles_matches_case_insensitively(pool: PgPool) {
    let a = make_user(&pool, "alice").await;
    let b = make_user(&pool, "bob_2").await;
    // Inputs arrive lowercased from parse_mentions; unknown handles drop out.
    let ids =
        notifications::resolve_handles(&pool, &["alice".into(), "bob_2".into(), "nope".into()])
            .await
            .unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&a) && ids.contains(&b));
}

#[sqlx::test(migrations = "./migrations")]
async fn unread_count_excludes_read(pool: PgPool) {
    let u = make_user(&pool, "carol").await;
    let actor = make_user(&pool, "dave").await;
    for (kind, read) in [("mention", false), ("assigned", false), ("comment", true)] {
        sqlx::query(
            r#"INSERT INTO notifications (id, user_id, actor_id, kind, title, read_at)
               VALUES ($1, $2, $3, $4, 'hi', CASE WHEN $5 THEN now() ELSE NULL END)"#,
        )
        .bind(Uuid::now_v7())
        .bind(u)
        .bind(actor)
        .bind(kind)
        .bind(read)
        .execute(&pool)
        .await
        .unwrap();
    }
    let unread: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL"#,
    )
    .bind(u)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unread, 2);
}
