//! F12 — personal API tokens: authenticate path (valid / expired / revoked /
//! scope-denied / suspended user), last_used_at tracking, secret hygiene.

use chrono::{Duration, Utc};
use sprintly_api::domain::api_tokens;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, 'Test User', 'x', 'member')
        "#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", id.simple()))
    .bind(format!("h{}", id.simple()))
    .execute(pool)
    .await
    .unwrap();
    id
}

fn scopes(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn valid_token_authenticates_and_touches_last_used(pool: PgPool) {
    let uid = make_user(&pool).await;
    let (row, secret) = api_tokens::create(&pool, uid, "ci-bot", &scopes(&["read"]), None)
        .await
        .unwrap();
    assert!(secret.starts_with("slt_"));
    assert!(row.last_used_at.is_none());

    let ident = api_tokens::authenticate(&pool, &secret, false)
        .await
        .unwrap();
    assert_eq!(ident.user_id, uid);
    assert_eq!(ident.role, "member");

    let listed = api_tokens::list(&pool, uid).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].last_used_at.is_some(), "last_used_at updated");
    // The list payload never carries the secret or its hash.
    let json = serde_json::to_string(&listed).unwrap();
    assert!(!json.contains("slt_"));
    assert!(!json.contains("hash"));
}

#[sqlx::test(migrations = "./migrations")]
async fn scope_denied_is_forbidden_not_unauthorized(pool: PgPool) {
    let uid = make_user(&pool).await;
    let (_, read_only) = api_tokens::create(&pool, uid, "ro", &scopes(&["read"]), None)
        .await
        .unwrap();
    let (_, writer) = api_tokens::create(&pool, uid, "rw", &scopes(&["write"]), None)
        .await
        .unwrap();

    // Read-only token: GET fine, write 403.
    assert!(api_tokens::authenticate(&pool, &read_only, false)
        .await
        .is_ok());
    assert!(matches!(
        api_tokens::authenticate(&pool, &read_only, true).await,
        Err(sprintly_api::AppError::Forbidden)
    ));
    // Write implies read.
    assert!(api_tokens::authenticate(&pool, &writer, false)
        .await
        .is_ok());
    assert!(api_tokens::authenticate(&pool, &writer, true).await.is_ok());
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_revoked_and_garbage_reject(pool: PgPool) {
    let uid = make_user(&pool).await;

    let (_, expired) = api_tokens::create(
        &pool,
        uid,
        "old",
        &scopes(&["read"]),
        Some(Utc::now() - Duration::hours(1)),
    )
    .await
    .unwrap();
    assert!(matches!(
        api_tokens::authenticate(&pool, &expired, false).await,
        Err(sprintly_api::AppError::Unauthorized)
    ));

    let (row, revoked) = api_tokens::create(&pool, uid, "gone", &scopes(&["read"]), None)
        .await
        .unwrap();
    api_tokens::revoke(&pool, uid, row.id).await.unwrap();
    assert!(matches!(
        api_tokens::authenticate(&pool, &revoked, false).await,
        Err(sprintly_api::AppError::Unauthorized)
    ));
    // Revoking twice → NotFound; revoking someone else's token → NotFound.
    assert!(api_tokens::revoke(&pool, uid, row.id).await.is_err());

    assert!(matches!(
        api_tokens::authenticate(
            &pool,
            "slt_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            false
        )
        .await,
        Err(sprintly_api::AppError::Unauthorized)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn suspended_user_tokens_stop_working(pool: PgPool) {
    let uid = make_user(&pool).await;
    let (_, secret) = api_tokens::create(&pool, uid, "t", &scopes(&["read"]), None)
        .await
        .unwrap();
    assert!(api_tokens::authenticate(&pool, &secret, false)
        .await
        .is_ok());

    sqlx::query(r#"UPDATE users SET status = 'suspended' WHERE id = $1"#)
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        api_tokens::authenticate(&pool, &secret, false).await,
        Err(sprintly_api::AppError::Unauthorized)
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn revoke_is_scoped_to_the_owner(pool: PgPool) {
    let alice = make_user(&pool).await;
    let bob = make_user(&pool).await;
    let (row, _) = api_tokens::create(&pool, alice, "hers", &scopes(&["read"]), None)
        .await
        .unwrap();
    // Bob can't revoke Alice's token.
    assert!(api_tokens::revoke(&pool, bob, row.id).await.is_err());
    assert_eq!(api_tokens::list(&pool, alice).await.unwrap().len(), 1);
}
