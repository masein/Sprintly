//! Integration tests for two-factor auth persistence (F11). The TOTP crypto
//! itself is unit-tested in `domain::totp`; here we exercise the DB lifecycle
//! against a real Postgres: enrol → activate gates logins, recovery codes are
//! single-use, and disable wipes everything.

use sprintly_api::{
    config::AuthConfig,
    domain::{password, totp, two_factor},
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
    let hash = password::hash(&cfg(), "hunter2-correct-horse").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, 'member')"#,
    )
    .bind(id)
    .bind(format!("u{}@example.test", id.simple()))
    .bind(format!("h{}", id.simple()))
    .bind("Test User")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test(migrations = "./migrations")]
async fn enrol_then_activate_gates_logins(pool: PgPool) {
    let user = make_user(&pool).await;

    // Fresh users have no second factor → login must not be gated.
    let s = two_factor::status(&pool, user).await.unwrap();
    assert!(!s.has_secret && !s.enabled);
    assert!(two_factor::secret_if_enabled(&pool, user)
        .await
        .unwrap()
        .is_none());

    // Start enrolment: a secret exists but logins are NOT yet gated.
    let secret = totp::generate_secret();
    two_factor::enroll_pending(&pool, user, &secret)
        .await
        .unwrap();
    let s = two_factor::status(&pool, user).await.unwrap();
    assert!(s.has_secret && !s.enabled, "pending, not active");
    assert!(
        two_factor::secret_if_enabled(&pool, user)
            .await
            .unwrap()
            .is_none(),
        "a pending secret must not gate login yet"
    );

    // Activate with recovery codes.
    let codes = totp::generate_recovery_codes(10);
    let hashes: Vec<String> = codes.iter().map(|c| totp::hash_recovery_code(c)).collect();
    assert!(two_factor::activate(&pool, user, &hashes).await.unwrap());

    // Now login IS gated, and the gating secret matches what we enrolled.
    let s = two_factor::status(&pool, user).await.unwrap();
    assert!(s.enabled);
    let gating = two_factor::secret_if_enabled(&pool, user).await.unwrap();
    assert_eq!(gating.as_deref(), Some(&secret[..]));

    // A code computed from that secret verifies — the login round-trip works.
    let now = 1_700_000_000u64;
    let code = totp::code_at(&secret, now);
    assert!(totp::verify(gating.as_ref().unwrap(), &code, now, 1));

    // Activating again is a no-op (nothing pending) — can't double-enable.
    assert!(!two_factor::activate(&pool, user, &hashes).await.unwrap());
}

#[sqlx::test(migrations = "./migrations")]
async fn recovery_codes_are_single_use(pool: PgPool) {
    let user = make_user(&pool).await;
    let secret = totp::generate_secret();
    two_factor::enroll_pending(&pool, user, &secret)
        .await
        .unwrap();

    let codes = totp::generate_recovery_codes(10);
    let hashes: Vec<String> = codes.iter().map(|c| totp::hash_recovery_code(c)).collect();
    two_factor::activate(&pool, user, &hashes).await.unwrap();

    // First use of a recovery code succeeds…
    assert!(two_factor::consume_recovery_code(&pool, user, &codes[0])
        .await
        .unwrap());
    // …the same code can never be used again.
    assert!(!two_factor::consume_recovery_code(&pool, user, &codes[0])
        .await
        .unwrap());
    // A different code still works, and formatting/casing is ignored.
    let messy = format!("  {}  ", codes[1].to_uppercase());
    assert!(two_factor::consume_recovery_code(&pool, user, &messy)
        .await
        .unwrap());
    // An unknown code is rejected.
    assert!(
        !two_factor::consume_recovery_code(&pool, user, "zzzzz-zzzzz")
            .await
            .unwrap()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn disable_wipes_second_factor(pool: PgPool) {
    let user = make_user(&pool).await;
    let secret = totp::generate_secret();
    two_factor::enroll_pending(&pool, user, &secret)
        .await
        .unwrap();
    let codes = totp::generate_recovery_codes(10);
    let hashes: Vec<String> = codes.iter().map(|c| totp::hash_recovery_code(c)).collect();
    two_factor::activate(&pool, user, &hashes).await.unwrap();

    two_factor::disable(&pool, user).await.unwrap();

    let s = two_factor::status(&pool, user).await.unwrap();
    assert!(!s.has_secret && !s.enabled, "everything cleared");
    assert!(two_factor::secret_if_enabled(&pool, user)
        .await
        .unwrap()
        .is_none());
    // Recovery codes are gone too.
    assert!(!two_factor::consume_recovery_code(&pool, user, &codes[2])
        .await
        .unwrap());
}
