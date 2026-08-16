//! Integration tests for session rotation + reuse detection.
//!
//! These need a real Postgres. `sqlx::test` spins up a fresh DB per test
//! against the DATABASE_URL env var, runs our migrations, and hands us a
//! PgPool. CI brings up a Postgres service for this; locally, `just up`
//! followed by `just test` does the same.

use sprintly_api::{
    config::AuthConfig,
    domain::{password, sessions, tokens},
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
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, $4, $5, 'member')
        "#,
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

/// Push every completed rotation `secs` into the past.
///
/// `rotate()` allows a short grace window after a rotation in which replaying
/// the just-rotated token is treated as a benign race (two tabs refreshing at
/// once) rather than theft. Tests that want the *theft* behaviour have to get
/// outside that window, and sleeping for real would make the suite slow and
/// flaky — so we age the row instead.
async fn age_rotations(pool: &PgPool, secs: i32) {
    sqlx::query(
        r#"UPDATE refresh_tokens
              SET rotated_at = now() - make_interval(secs => $1::int)
            WHERE rotated_to IS NOT NULL"#,
    )
    .bind(secs)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_rotates_and_invalidates_old(pool: PgPool) {
    let cfg = cfg();
    let user_id = make_user(&pool).await;

    let issued = sessions::create(&pool, &cfg, user_id, Some("test"), None)
        .await
        .unwrap();
    let original = issued.refresh.plaintext.clone();

    // Rotate once — produces a new refresh.
    let new_one = match sessions::rotate(&pool, &cfg, &original).await.unwrap() {
        sessions::RotateOutcome::Rotated { refresh, .. } => refresh,
    };

    // The new token rotates again (still valid).
    sessions::rotate(&pool, &cfg, &new_one.plaintext)
        .await
        .unwrap();

    // The original cannot be used a second time — that's reuse. Age the
    // rotation past the grace window first, so this is the theft case and not
    // the two-tabs-raced case (covered by its own test below).
    age_rotations(&pool, 60).await;
    let reused = sessions::rotate(&pool, &cfg, &original).await;
    assert!(reused.is_err(), "old token must not rotate twice");
}

#[sqlx::test(migrations = "./migrations")]
async fn reuse_revokes_entire_session_family(pool: PgPool) {
    let cfg = cfg();
    let user_id = make_user(&pool).await;

    let issued = sessions::create(&pool, &cfg, user_id, Some("test"), None)
        .await
        .unwrap();
    let original = issued.refresh.plaintext.clone();

    // Rotate legitimately.
    let (t2, session_id) = match sessions::rotate(&pool, &cfg, &original).await.unwrap() {
        sessions::RotateOutcome::Rotated {
            refresh,
            session_id,
            ..
        } => (refresh, session_id),
    };

    // Attacker (or buggy client) replays the original, long enough after the
    // rotation that it can't be a refresh race. This should burn the entire
    // session — even t2 must stop working.
    age_rotations(&pool, 60).await;
    let replay = sessions::rotate(&pool, &cfg, &original).await;
    assert!(replay.is_err(), "stale token must be rejected");

    let session_still_live = sessions::session_is_live(&pool, session_id).await.unwrap();
    assert!(
        !session_still_live,
        "session must be revoked once reuse is detected"
    );

    // And the leaf token from the legitimate rotation is now unusable.
    let leaf_after = sessions::rotate(&pool, &cfg, &t2.plaintext).await;
    assert!(
        leaf_after.is_err(),
        "leaf token must be revoked alongside its family"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_refresh_token_is_unauthorized(pool: PgPool) {
    let cfg = cfg();
    // Random plaintext, not in the DB.
    let random = tokens::mint_refresh();
    let result = sessions::rotate(&pool, &cfg, &random.plaintext).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn revoking_session_kills_refresh(pool: PgPool) {
    let cfg = cfg();
    let user_id = make_user(&pool).await;
    let issued = sessions::create(&pool, &cfg, user_id, Some("test"), None)
        .await
        .unwrap();

    sessions::revoke(&pool, issued.session_id, "logout")
        .await
        .unwrap();

    let result = sessions::rotate(&pool, &cfg, &issued.refresh.plaintext).await;
    assert!(result.is_err(), "logged-out session must not rotate");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_refresh_race_inside_the_grace_window_is_not_treated_as_theft(pool: PgPool) {
    let cfg = cfg();
    let user_id = make_user(&pool).await;

    let issued = sessions::create(&pool, &cfg, user_id, Some("test"), None)
        .await
        .unwrap();
    let original = issued.refresh.plaintext.clone();

    // Tab A wins the race.
    let (winner, session_id) = match sessions::rotate(&pool, &cfg, &original).await.unwrap() {
        sessions::RotateOutcome::Rotated {
            refresh,
            session_id,
            ..
        } => (refresh, session_id),
    };

    // Tab B arrives moments later still holding the original. This is the bug
    // QA hit as "logged out after ~5 minutes": the old code burned the family
    // here, so both tabs lost their session. It should instead be handed its
    // own leaf.
    let loser = match sessions::rotate(&pool, &cfg, &original).await {
        Ok(sessions::RotateOutcome::Rotated { refresh, .. }) => refresh,
        Err(e) => panic!("a same-instant replay should be tolerated, got {e:?}"),
    };
    assert_ne!(
        loser.plaintext, winner.plaintext,
        "the replayer needs its own token, not a copy of the winner's"
    );

    // The session survived…
    assert!(
        sessions::session_is_live(&pool, session_id).await.unwrap(),
        "a refresh race must not revoke the session"
    );

    // …and both tabs can carry on independently.
    assert!(
        sessions::rotate(&pool, &cfg, &winner.plaintext)
            .await
            .is_ok(),
        "the winner's token should still rotate"
    );
    assert!(
        sessions::rotate(&pool, &cfg, &loser.plaintext)
            .await
            .is_ok(),
        "the replayer's token should still rotate"
    );

    // The grace is a window, not an amnesty: the same replay after it closes
    // is theft again, and burns the family.
    age_rotations(&pool, 60).await;
    assert!(
        sessions::rotate(&pool, &cfg, &original).await.is_err(),
        "replay outside the window must be rejected"
    );
    assert!(
        !sessions::session_is_live(&pool, session_id).await.unwrap(),
        "late replay must still revoke the session family"
    );
}
