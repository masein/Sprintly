//! Job worker claim semantics against a real Postgres: stale claims (a worker
//! that died mid-run) are reclaimed, fresh claims are left alone, and an
//! interrupted job still runs out of attempts.

use sprintly_api::{
    config::EmailConfig,
    infra::email,
    jobs::{run_one, MailCtx},
};
use sqlx::PgPool;
use uuid::Uuid;

fn mail() -> MailCtx {
    MailCtx {
        mailer: email::build(&EmailConfig {
            smtp_url: None,
            mail_from: "Sprintly <noreply@sprintly.test>".into(),
        }),
        public_url: "http://localhost:8080".into(),
    }
}

/// A `scan_achievements` job (harmless on an empty DB) claimed `claimed_ago`
/// seconds ago and never finished.
async fn claimed_job(pool: &PgPool, claimed_ago: i32, attempts: i32) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO jobs (id, kind, run_at, attempts, claimed_at)
           VALUES ($1, 'scan_achievements', now() - interval '1 day', $2,
                   now() - make_interval(secs => $3::int))"#,
    )
    .bind(id)
    .bind(attempts)
    .bind(claimed_ago)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn state(pool: &PgPool, id: Uuid) -> (bool, i32, Option<String>) {
    sqlx::query_as("SELECT finished_at IS NOT NULL, attempts, last_error FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn stale_claims_are_reclaimed_and_run(pool: PgPool) {
    let key = [0u8; 32];
    let id = claimed_job(&pool, 3 * 60 * 60, 0).await; // claimed 3h ago
    assert!(run_one(&pool, &key, &mail()).await.unwrap(), "picked up");
    let (finished, attempts, err) = state(&pool, id).await;
    assert!(finished, "the reclaimed job ran to completion");
    assert_eq!(attempts, 1, "the interrupted run counted as an attempt");
    assert!(err.unwrap_or_default().contains("interrupted"));
}

#[sqlx::test(migrations = "./migrations")]
async fn fresh_claims_are_left_alone(pool: PgPool) {
    let key = [0u8; 32];
    let id = claimed_job(&pool, 60, 0).await; // claimed a minute ago: still running
    assert!(
        !run_one(&pool, &key, &mail()).await.unwrap(),
        "nothing to do"
    );
    let (finished, attempts, _) = state(&pool, id).await;
    assert!(!finished);
    assert_eq!(attempts, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_that_keeps_dying_runs_out_of_attempts(pool: PgPool) {
    let key = [0u8; 32];
    let id = claimed_job(&pool, 3 * 60 * 60, 9).await; // max_attempts defaults to 10
    assert!(run_one(&pool, &key, &mail()).await.unwrap());
    let (finished, attempts, err) = state(&pool, id).await;
    assert!(finished, "given up rather than retried forever");
    assert_eq!(attempts, 10);
    assert!(err.unwrap_or_default().contains("interrupted"));
    // …and it was NOT re-run: no successor scan was enqueued.
    let successors: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind = 'scan_achievements' AND id <> $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(successors, 0);
}
