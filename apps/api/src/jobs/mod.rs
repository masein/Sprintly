//! In-process background workers.
//!
//! Design:
//!   * One Tokio task per replica polls the `jobs` table on a tick.
//!   * Each iteration claims one row via `FOR UPDATE SKIP LOCKED` so multiple
//!     replicas don't race for the same job.
//!   * Job kinds dispatch to a Rust function; unknown kinds are marked done
//!     with `last_error = "unknown kind"` so they don't get retried forever.
//!   * On success: `finished_at = now()`. On failure: bump `attempts`,
//!     unclaim, exponential backoff via `run_at`.
//!
//! Built-in seed: on boot, we ensure a single `scan_achievements` row exists
//! that re-enqueues itself every 5 minutes after each run. There's no UI for
//! creating jobs in v1 — they're all internal.

use std::time::Duration;

use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::achievements;

const POLL_INTERVAL: Duration = Duration::from_secs(15);
const ACHIEVEMENT_SCAN_EVERY_SECS: i64 = 300;

/// Spawn the worker on the runtime. Returns immediately. The task runs for
/// the lifetime of the process; on shutdown the runtime cancels it.
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        if let Err(e) = ensure_seed_jobs(&pool).await {
            warn!(error = %e, "jobs: seed failed");
        }
        loop {
            match run_one(&pool).await {
                Ok(ran) => {
                    if !ran {
                        // No runnable job — sleep before polling again.
                        tokio::time::sleep(POLL_INTERVAL).await;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "jobs: iteration error");
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            }
        }
    });
}

async fn ensure_seed_jobs(pool: &PgPool) -> anyhow::Result<()> {
    // Insert the achievement scan if no row of that kind exists yet.
    let n: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM jobs WHERE kind = 'scan_achievements' AND finished_at IS NULL"#,
    )
    .fetch_one(pool)
    .await?;
    if n == 0 {
        sqlx::query(
            r#"
            INSERT INTO jobs (id, kind, run_at)
            VALUES ($1, 'scan_achievements', now())
            "#,
        )
        .bind(Uuid::now_v7())
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Claim + run one job. Returns true if a job was processed.
async fn run_one(pool: &PgPool) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;

    // Claim a single runnable job. SKIP LOCKED so concurrent workers don't
    // serialize on the same row.
    let row: Option<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, kind
        FROM   jobs
        WHERE  finished_at IS NULL AND claimed_at IS NULL AND run_at <= now()
        ORDER  BY run_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT  1
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some((id, kind)) = row else {
        tx.commit().await?;
        return Ok(false);
    };

    sqlx::query("UPDATE jobs SET claimed_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // Dispatch.
    let result = match kind.as_str() {
        "scan_achievements" => run_scan_achievements(pool).await,
        "create_backup" => run_create_backup(pool, id).await,
        other => Err(anyhow::anyhow!("unknown job kind: {other}")),
    };

    match result {
        Ok(()) => finish_ok(pool, id, &kind).await?,
        Err(e) => finish_err(pool, id, &e.to_string()).await?,
    }
    Ok(true)
}

/// Run `pg_dump` against the configured DATABASE_URL and stream the dump
/// into MinIO. We expect the runtime image to have `pg_dump` and
/// `curl` available. The job rows is updated as we move through stages so
/// the admin UI sees status transitions in real time.
async fn run_create_backup(pool: &PgPool, job_id: Uuid) -> anyhow::Result<()> {
    use std::process::Stdio;
    use tokio::process::Command;

    // Read the backup row id from the job payload.
    let payload: serde_json::Value = sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;
    let backup_id: Uuid = payload
        .get("backup_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("backup_id missing/invalid in payload"))?;

    sqlx::query("UPDATE backups SET status = 'running', started_at = now() WHERE id = $1")
        .bind(backup_id)
        .execute(pool)
        .await?;

    // Read env directly — the worker doesn't have a Config handle.
    let database_url = std::env::var("DATABASE_URL")?;
    let minio_bucket = std::env::var("MINIO_BUCKET")?;
    let storage_key = format!(
        "backups/{}/{}.dump",
        chrono::Utc::now().format("%Y-%m-%d"),
        backup_id
    );
    let tmp_path = format!("/tmp/sprintly-backup-{backup_id}.dump");

    // Try to run pg_dump. If the binary isn't present (dev image without
    // postgresql-client), mark this attempt failed with a helpful error
    // instead of panicking.
    let dump = Command::new("pg_dump")
        .args([
            "--format=custom",
            "--no-owner",
            "--no-acl",
            "-Z",
            "6",
            "-f",
            &tmp_path,
            &database_url,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await;
    let out = match dump {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("pg_dump invocation failed: {e}");
            mark_backup_failed(pool, backup_id, &msg).await?;
            return Err(anyhow::anyhow!(msg));
        }
    };
    if !out.status.success() {
        let msg = format!(
            "pg_dump exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        mark_backup_failed(pool, backup_id, &msg).await?;
        return Err(anyhow::anyhow!(msg));
    }

    let meta = tokio::fs::metadata(&tmp_path).await?;
    let size = meta.len() as i64;

    // Upload to MinIO via `mc`-free path: use the presigner we already have.
    // Build the URL and PUT with curl. (We can't easily reuse the Rust signer
    // here without a Config handle; reading env keeps the worker decoupled.)
    let endpoint = std::env::var("MINIO_ENDPOINT")?;
    let access_key = std::env::var("MINIO_ROOT_USER")?;
    let secret_key = std::env::var("MINIO_ROOT_PASSWORD")?;
    let region = std::env::var("MINIO_REGION").unwrap_or_else(|_| "us-east-1".into());

    let cfg = crate::config::MinioConfig {
        endpoint: endpoint.clone(),
        public_endpoint: endpoint, // internal upload — same host
        access_key,
        secret_key,
        bucket: minio_bucket.clone(),
        region,
    };
    let signer = crate::infra::s3::Presigner::new(&cfg);
    let url = signer.put(&storage_key, "application/octet-stream", 900);

    let upload = Command::new("curl")
        .args([
            "--fail-with-body",
            "-sS",
            "-X",
            "PUT",
            "--data-binary",
            &format!("@{tmp_path}"),
            "-H",
            "Content-Type: application/octet-stream",
            &url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;
    // Tidy the temp file regardless of upload result.
    let _ = tokio::fs::remove_file(&tmp_path).await;

    let upload_out = match upload {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("curl invocation failed: {e}");
            mark_backup_failed(pool, backup_id, &msg).await?;
            return Err(anyhow::anyhow!(msg));
        }
    };
    if !upload_out.status.success() {
        let msg = format!(
            "minio upload exited {}: {}",
            upload_out.status,
            String::from_utf8_lossy(&upload_out.stderr)
        );
        mark_backup_failed(pool, backup_id, &msg).await?;
        return Err(anyhow::anyhow!(msg));
    }

    sqlx::query(
        r#"
        UPDATE backups SET
            status = 'done',
            finished_at = now(),
            size_bytes = $2,
            storage_key = $3
        WHERE id = $1
        "#,
    )
    .bind(backup_id)
    .bind(size)
    .bind(&storage_key)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_backup_failed(pool: &PgPool, id: Uuid, msg: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE backups SET status = 'failed', finished_at = now(), error = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(msg)
    .execute(pool)
    .await?;
    Ok(())
}

async fn run_scan_achievements(pool: &PgPool) -> anyhow::Result<()> {
    let batches = achievements::scan_all(pool).await?;
    let mut inserted_total = 0u64;
    for (code, candidates) in batches {
        let n = achievements::award_batch(pool, code, &candidates).await?;
        if n > 0 {
            inserted_total += n;
            info!(code, awarded = n, "achievements: granted");
        }
    }
    if inserted_total > 0 {
        info!(total = inserted_total, "achievement scan: done");
    }
    Ok(())
}

async fn finish_ok(pool: &PgPool, id: Uuid, kind: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE jobs SET finished_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    // Re-enqueue the achievement scan after a 5-minute cooldown.
    if kind == "scan_achievements" {
        sqlx::query(
            r#"
            INSERT INTO jobs (id, kind, run_at)
            VALUES ($1, 'scan_achievements', now() + ($2::int || ' seconds')::interval)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(ACHIEVEMENT_SCAN_EVERY_SECS as i32)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn finish_err(pool: &PgPool, id: Uuid, msg: &str) -> anyhow::Result<()> {
    // Bump attempts. If at max, finish with the error and stop retrying.
    let row: (i32, i32) = sqlx::query_as("SELECT attempts, max_attempts FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    let attempts = row.0 + 1;
    if attempts >= row.1 {
        sqlx::query(
            r#"
            UPDATE jobs
               SET attempts = $2,
                   last_error = $3,
                   finished_at = now(),
                   claimed_at = NULL
             WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(attempts)
        .bind(msg)
        .execute(pool)
        .await?;
    } else {
        // Exponential backoff: 2^attempts seconds, capped at 1h.
        let backoff_secs = (2_i64.pow(attempts.min(12) as u32)).min(3600);
        sqlx::query(
            r#"
            UPDATE jobs
               SET attempts = $2,
                   last_error = $3,
                   claimed_at = NULL,
                   run_at = now() + ($4::int || ' seconds')::interval
             WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(attempts)
        .bind(msg)
        .bind(backoff_secs as i32)
        .execute(pool)
        .await?;
    }
    Ok(())
}
