//! Integration tests for OIDC claim→user mapping (F10). The signature/PKCE/
//! flow logic is unit-tested in `domain::oidc`; here we exercise create-or-link
//! against a real Postgres.

use sprintly_api::{
    config::AuthConfig,
    domain::{oidc::IdClaims, password},
};
use sqlx::PgPool;
use uuid::Uuid;

const ISSUER: &str = "https://idp.test";

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

fn claims(sub: &str, email: Option<&str>, verified: bool) -> IdClaims {
    IdClaims {
        sub: sub.into(),
        iss: ISSUER.into(),
        exp: 9_999_999_999,
        email: email.map(String::from),
        email_verified: verified,
        name: Some("Federated User".into()),
        nonce: Some("n".into()),
    }
}

async fn make_local_user(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "hunter2-correct-horse").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, 'member')"#,
    )
    .bind(id)
    .bind(email)
    .bind(format!("h{}", id.simple()))
    .bind("Local User")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test(migrations = "./migrations")]
async fn first_login_creates_user_and_is_idempotent(pool: PgPool) {
    let (id, role) = sprintly_api::domain::oidc::upsert_user(
        &pool,
        &cfg(),
        ISSUER,
        &claims("sub-1", Some("new@idp.test"), true),
    )
    .await
    .unwrap();
    assert_eq!(role, "member");

    // Created with the federated identity + active status.
    let (status, oidc_sub): (String, Option<String>) =
        sqlx::query_as("SELECT status, oidc_subject FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "active");
    assert_eq!(oidc_sub.as_deref(), Some("sub-1"));

    // Logging in again resolves to the SAME user — no duplicate.
    let (id2, _) = sprintly_api::domain::oidc::upsert_user(
        &pool,
        &cfg(),
        ISSUER,
        &claims("sub-1", Some("new@idp.test"), true),
    )
    .await
    .unwrap();
    assert_eq!(id, id2);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE oidc_subject = 'sub-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn verified_email_links_to_existing_local_account(pool: PgPool) {
    let existing = make_local_user(&pool, "dev@idp.test").await;

    let (id, _) = sprintly_api::domain::oidc::upsert_user(
        &pool,
        &cfg(),
        ISSUER,
        &claims("sub-link", Some("dev@idp.test"), true),
    )
    .await
    .unwrap();

    // Linked onto the existing row, not a new user.
    assert_eq!(id, existing);
    let oidc_sub: Option<String> =
        sqlx::query_scalar("SELECT oidc_subject FROM users WHERE id = $1")
            .bind(existing)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(oidc_sub.as_deref(), Some("sub-link"));
}

#[sqlx::test(migrations = "./migrations")]
async fn unverified_email_does_not_take_over_an_account(pool: PgPool) {
    make_local_user(&pool, "victim@idp.test").await;

    let err = sprintly_api::domain::oidc::upsert_user(
        &pool,
        &cfg(),
        ISSUER,
        &claims("sub-attacker", Some("victim@idp.test"), false),
    )
    .await;
    assert!(
        err.is_err(),
        "unverified email must not link to a local account"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_email_is_rejected(pool: PgPool) {
    let err = sprintly_api::domain::oidc::upsert_user(
        &pool,
        &cfg(),
        ISSUER,
        &claims("sub-x", None, true),
    )
    .await;
    assert!(err.is_err());
}
