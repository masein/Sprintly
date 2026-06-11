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
/// the lifetime of the process; on shutdown the runtime cancels it. The vault
/// master key is needed to decrypt webhook signing secrets at delivery time.
pub fn spawn(pool: PgPool, vault_master_key: [u8; 32]) {
    tokio::spawn(async move {
        if let Err(e) = ensure_seed_jobs(&pool).await {
            warn!(error = %e, "jobs: seed failed");
        }
        loop {
            match run_one(&pool, &vault_master_key).await {
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
async fn run_one(pool: &PgPool, vault_master_key: &[u8; 32]) -> anyhow::Result<bool> {
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
        "deliver_webhook" => run_deliver_webhook(pool, id, vault_master_key).await,
        "push_commit_status" => run_push_commit_status(pool, id, vault_master_key).await,
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

/// Deliver one webhook: decrypt its signing secret, HMAC-sign the body, POST it
/// via curl, and record the attempt. Returns `Err` on a non-2xx / transport
/// failure so the worker retries it with backoff (up to `max_attempts`).
async fn run_deliver_webhook(
    pool: &PgPool,
    job_id: Uuid,
    vault_master_key: &[u8; 32],
) -> anyhow::Result<()> {
    use crate::domain::vault::{decrypt, ProjectKey};
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let payload: serde_json::Value = sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;
    let webhook_id: Uuid = payload
        .get("webhook_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("deliver_webhook: bad webhook_id in payload"))?;
    let event = payload.get("event").and_then(|v| v.as_str()).unwrap_or("");
    let body = payload.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let attempt: i32 = sqlx::query_scalar("SELECT attempts + 1 FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;

    // (url, project_id, secret_ciphertext, secret_nonce)
    type HookRow = (String, Uuid, Option<Vec<u8>>, Option<Vec<u8>>);
    let hook: Option<HookRow> = sqlx::query_as(
        r#"SELECT url, project_id, secret_ciphertext, secret_nonce
           FROM webhooks WHERE id = $1 AND deleted_at IS NULL AND active = true"#,
    )
    .bind(webhook_id)
    .fetch_optional(pool)
    .await?;
    // Webhook deleted/deactivated/unconfigured since enqueue — drop the job.
    let Some((url, project_id, Some(ct), Some(nonce))) = hook else {
        return Ok(());
    };

    let key = ProjectKey::derive(vault_master_key, project_id, 1);
    let secret =
        decrypt(&key, &ct, &nonce, webhook_id.as_bytes()).map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut mac =
        Hmac::<Sha256>::new_from_slice(&secret).map_err(|e| anyhow::anyhow!("hmac key: {e}"))?;
    mac.update(body.as_bytes());
    let sig = hex_encode(&mac.finalize().into_bytes());

    let out = tokio::process::Command::new("curl")
        .args(["-sS", "-X", "POST", &url])
        .args(["-H", "Content-Type: application/json"])
        .args(["-H", &format!("X-Sprintly-Event: {event}")])
        .args(["-H", &format!("X-Sprintly-Signature: sha256={sig}")])
        .args(["--data-binary", body])
        .args(["--max-time", "10"])
        .args(["-o", "/dev/null", "-w", "%{http_code}"])
        .output()
        .await?;
    let code: i32 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    let ok = (200..300).contains(&code);
    let err = (!ok).then(|| {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.trim().is_empty() {
            format!("HTTP {code}")
        } else {
            format!("HTTP {code}: {}", stderr.trim())
        }
    });

    sqlx::query(
        r#"INSERT INTO webhook_deliveries (id, webhook_id, event, status_code, ok, error, attempt)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(Uuid::now_v7())
    .bind(webhook_id)
    .bind(event)
    .bind(code)
    .bind(ok)
    .bind(err.as_deref())
    .bind(attempt)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE webhooks SET last_delivery_at = now(), last_status = $2 WHERE id = $1")
        .bind(webhook_id)
        .bind(code)
        .execute(pool)
        .await?;

    if ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("webhook {webhook_id} POST returned {code}"))
    }
}

/// Push the task's state to every status-enabled provider connection of its
/// project, as a commit status on each linked SHA (ADR 0001). The job
/// payload is just the task id — state is read at run time, so a burst of
/// moves collapses to the final truth. Any failed POST returns `Err` so the
/// worker retries the batch; provider status APIs are idempotent.
async fn run_push_commit_status(
    pool: &PgPool,
    job_id: Uuid,
    vault_master_key: &[u8; 32],
) -> anyhow::Result<()> {
    use crate::domain::{
        git_providers::{self, Provider},
        integrations,
    };

    let payload: serde_json::Value = sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;
    let task_id: Uuid = payload
        .get("task_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("push_commit_status: bad task_id in payload"))?;

    // Task vanished since enqueue — drop the job.
    let task: Option<(String, String, Uuid)> = sqlx::query_as(
        r#"SELECT key, status, project_id FROM tasks WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;
    let Some((task_key, task_status, project_id)) = task else {
        return Ok(());
    };

    let state = git_providers::task_status_to_state(&task_status);
    let public_url =
        std::env::var("SPRINTLY_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let target_url = format!("{}/tasks/{task_key}", public_url.trim_end_matches('/'));
    let context = format!("sprintly/{task_key}");
    let human = match task_status.as_str() {
        "done" => "done".to_string(),
        s => s.replace('_', " "),
    };
    let description = format!("{task_key} is {human}");

    // Cap the fan-out: the most recent SHAs are the ones anyone looks at.
    let shas: Vec<String> = sqlx::query_scalar(
        r#"SELECT DISTINCT ON (sha) sha FROM git_links
           WHERE task_id = $1 AND sha IS NOT NULL
           ORDER BY sha, created_at DESC
           LIMIT 20"#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;

    type IntegrationRow = (Uuid, String, String, Option<String>);
    let connections: Vec<IntegrationRow> = sqlx::query_as(
        r#"SELECT id, provider, repo, base_url FROM git_integrations
           WHERE project_id = $1 AND status_enabled AND api_token_ct IS NOT NULL"#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    let mut failures = Vec::new();
    for (integration_id, provider_str, repo, base_url) in &connections {
        let Some(provider) = Provider::parse(provider_str) else {
            continue;
        };
        let Some(token) =
            integrations::decrypt_api_token(pool, vault_master_key, *integration_id).await?
        else {
            continue;
        };
        for sha in &shas {
            let req = git_providers::status_request(
                provider,
                base_url.as_deref(),
                repo,
                &token,
                sha,
                state,
                &context,
                &description,
                &target_url,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

            let mut cmd = tokio::process::Command::new("curl");
            cmd.args(["-sS", "-X", req.method, &req.url]);
            for (name, value) in &req.headers {
                cmd.args(["-H", &format!("{name}: {value}")]);
            }
            let out = cmd
                .args(["--data-binary", &req.body])
                .args(["--max-time", "10"])
                .args(["-o", "/dev/null", "-w", "%{http_code}"])
                .output()
                .await?;
            let code: i32 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
            if !(200..300).contains(&code) {
                // Status code only — never echo the request (it carries the token).
                failures.push(format!("{provider_str} {repo}@{sha}: HTTP {code}"));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "commit status push failed: {}",
            failures.join("; ")
        ))
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
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
