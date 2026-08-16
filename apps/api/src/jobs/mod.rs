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

use std::{sync::Arc, time::Duration};

use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::{achievements, backups, email_prefs};
use crate::infra::email::{self, Mailer};

/// What the worker needs to send mail: a mailer and the public base URL that
/// goes into every link. Cloned per job — both are cheap.
#[derive(Clone)]
pub struct MailCtx {
    pub mailer: Arc<dyn Mailer>,
    pub public_url: String,
}

const POLL_INTERVAL: Duration = Duration::from_secs(15);
const ACHIEVEMENT_SCAN_EVERY_SECS: i64 = 300;
/// How often the worker materialises due recurring templates (F9).
const TEMPLATE_MATERIALIZE_EVERY_SECS: i64 = 300;
/// How often the digest worker looks for users whose local send-hour arrived.
/// Five minutes keeps "08:00" honest to within a coffee's width, and the query
/// is a single indexed scan that finds nothing most of the time.
const DIGEST_SWEEP_EVERY_SECS: i64 = 300;
/// How often the worker prunes old backups per the retention policy (F15).
const BACKUP_RETENTION_EVERY_SECS: i64 = 21_600; // 6h

/// Spawn the worker on the runtime. Returns immediately. The task runs for
/// the lifetime of the process; on shutdown the runtime cancels it. The vault
/// master key is needed to decrypt webhook signing secrets at delivery time.
pub fn spawn(pool: PgPool, vault_master_key: [u8; 32], mail: MailCtx) {
    tokio::spawn(async move {
        if let Err(e) = ensure_seed_jobs(&pool).await {
            warn!(error = %e, "jobs: seed failed");
        }
        loop {
            match run_one(&pool, &vault_master_key, &mail).await {
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
    // One self-re-enqueuing row per periodic kind. Backup scheduling and
    // retention are opt-in via env, so they're only seeded when configured.
    let mut periodic = vec![
        "scan_achievements",
        "materialize_templates",
        "notification_digest",
    ];
    if backups::schedule_secs().is_some() {
        periodic.push("scheduled_backup");
    }
    if backups::RetentionPolicy::from_env().is_active() {
        periodic.push("backup_retention");
    }
    for kind in periodic {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = $1 AND finished_at IS NULL")
                .bind(kind)
                .fetch_one(pool)
                .await?;
        if n == 0 {
            sqlx::query("INSERT INTO jobs (id, kind, run_at) VALUES ($1, $2, now())")
                .bind(Uuid::now_v7())
                .bind(kind)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Claim + run one job. Returns true if a job was processed.
async fn run_one(
    pool: &PgPool,
    vault_master_key: &[u8; 32],
    mail: &MailCtx,
) -> anyhow::Result<bool> {
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
        "materialize_templates" => run_materialize_templates(pool).await,
        "scheduled_backup" => run_scheduled_backup(pool).await,
        "backup_retention" => run_backup_retention(pool).await,
        "send_notification_email" => run_send_notification_email(pool, id, mail).await,
        "notification_digest" => run_notification_digest(pool, mail).await,
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

/// Scheduled backup tick (F15): create a `backups` row and enqueue the same
/// `create_backup` job a manual trigger would, so the two paths converge.
async fn run_scheduled_backup(pool: &PgPool) -> anyhow::Result<()> {
    let backup_id = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    sqlx::query(r#"INSERT INTO backups (id, requested_by, status) VALUES ($1, NULL, 'pending')"#)
        .bind(backup_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        r#"INSERT INTO jobs (id, kind, payload, run_at)
           VALUES ($1, 'create_backup', jsonb_build_object('backup_id', $2::text), now())"#,
    )
    .bind(Uuid::now_v7())
    .bind(backup_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    info!(%backup_id, "jobs: scheduled backup enqueued");
    Ok(())
}

/// Retention prune (F15): delete completed backups beyond the configured policy
/// — both the MinIO object and the row. Pruning is skipped entirely when no
/// policy is configured (the historical keep-everything behaviour).
async fn run_backup_retention(pool: &PgPool) -> anyhow::Result<()> {
    let policy = backups::RetentionPolicy::from_env();
    if !policy.is_active() {
        return Ok(());
    }
    let done = backups::load_done_backups(pool).await?;
    let prunable = backups::select_prunable(&done, &policy, chrono::Utc::now());
    if prunable.is_empty() {
        return Ok(());
    }

    let cfg = minio_cfg_from_env()?;
    let signer = crate::infra::s3::Presigner::new(&cfg);
    let mut pruned = 0u32;
    for b in prunable {
        if let Some(key) = &b.storage_key {
            // Best-effort object delete; we drop the row regardless so a
            // missing/half-deleted object can't wedge retention forever.
            let url = signer.delete(key, 300);
            let out = tokio::process::Command::new("curl")
                .args(["-sS", "-X", "DELETE", "-o", "/dev/null", &url])
                .output()
                .await;
            if let Err(e) = &out {
                warn!(error = %e, key, "jobs: backup object delete failed");
            }
        }
        backups::delete_backup_row(pool, b.id).await?;
        pruned += 1;
    }
    info!(pruned, "jobs: backup retention pruned old backups");
    Ok(())
}

/// MinIO config for server-side object access from the worker. Reads env (the
/// worker has no Config handle) and points the presigner at the internal
/// endpoint, mirroring `run_create_backup`.
fn minio_cfg_from_env() -> anyhow::Result<crate::config::MinioConfig> {
    let endpoint = std::env::var("MINIO_ENDPOINT")?;
    Ok(crate::config::MinioConfig {
        public_endpoint: endpoint.clone(),
        endpoint,
        access_key: std::env::var("MINIO_ROOT_USER")?,
        secret_key: std::env::var("MINIO_ROOT_PASSWORD")?,
        bucket: std::env::var("MINIO_BUCKET")?,
        region: std::env::var("MINIO_REGION").unwrap_or_else(|_| "us-east-1".into()),
    })
}

/// Spawn a task for every recurring template whose next run is due (F9).
async fn run_materialize_templates(pool: &PgPool) -> anyhow::Result<()> {
    let made = crate::domain::templates::materialise_due(pool, chrono::Utc::now()).await?;
    if !made.is_empty() {
        info!(
            count = made.len(),
            "templates: materialised recurring tasks"
        );
    }
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

/// Deliver one webhook (ADR 0002). The body + headers depend on the row's
/// `target_type`: `outbound` signs `{event,data}` with the stored secret;
/// `slack`/`discord` POST a formatted message to the URL (no signature). POSTs
/// via curl and records the attempt; returns `Err` on a non-2xx / transport
/// failure so the worker retries it with backoff (up to `max_attempts`).
async fn run_deliver_webhook(
    pool: &PgPool,
    job_id: Uuid,
    vault_master_key: &[u8; 32],
) -> anyhow::Result<()> {
    use crate::domain::{
        chat_adapters,
        vault::{decrypt, ProjectKey},
    };
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
    let data = payload
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let attempt: i32 = sqlx::query_scalar("SELECT attempts + 1 FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;

    // (url, project_id, secret_ciphertext, secret_nonce, target_type)
    type HookRow = (String, Uuid, Option<Vec<u8>>, Option<Vec<u8>>, String);
    let hook: Option<HookRow> = sqlx::query_as(
        r#"SELECT url, project_id, secret_ciphertext, secret_nonce, target_type
           FROM webhooks WHERE id = $1 AND deleted_at IS NULL AND active = true"#,
    )
    .bind(webhook_id)
    .fetch_optional(pool)
    .await?;
    // Webhook deleted/deactivated since enqueue — drop the job.
    let Some((url, project_id, ct, nonce, target_type)) = hook else {
        return Ok(());
    };

    // Build (body, headers) per target type.
    let mut headers: Vec<(String, String)> =
        vec![("Content-Type".into(), "application/json".into())];
    let body: String = if target_type == "outbound" {
        // Generic signed delivery. Unconfigured (no secret) → drop.
        let (Some(ct), Some(nonce)) = (ct, nonce) else {
            return Ok(());
        };
        let key = ProjectKey::derive(vault_master_key, project_id, 1);
        let secret = decrypt(&key, &ct, &nonce, webhook_id.as_bytes())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let body = serde_json::json!({ "event": event, "data": data }).to_string();
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret)
            .map_err(|e| anyhow::anyhow!("hmac key: {e}"))?;
        mac.update(body.as_bytes());
        let sig = hex_encode(&mac.finalize().into_bytes());
        headers.push(("X-Sprintly-Event".into(), event.to_string()));
        headers.push(("X-Sprintly-Signature".into(), format!("sha256={sig}")));
        body
    } else {
        // Chat target: format the message; the URL is the credential.
        let public_url =
            std::env::var("SPRINTLY_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let link = chat_adapters::task_key(&data)
            .map(|k| format!("{}/tasks/{k}", public_url.trim_end_matches('/')));
        chat_adapters::format_message(&target_type, event, &data, link.as_deref())
            .ok_or_else(|| anyhow::anyhow!("unknown webhook target_type: {target_type}"))?
    };

    let mut cmd = tokio::process::Command::new("curl");
    cmd.args(["-sS", "-X", "POST", &url]);
    for (name, value) in &headers {
        cmd.args(["-H", &format!("{name}: {value}")]);
    }
    let out = cmd
        .args(["--data-binary", &body])
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
    // Periodic kinds re-enqueue themselves after a cooldown.
    let cooldown = match kind {
        "scan_achievements" => Some(ACHIEVEMENT_SCAN_EVERY_SECS),
        "materialize_templates" => Some(TEMPLATE_MATERIALIZE_EVERY_SECS),
        "backup_retention" => Some(BACKUP_RETENTION_EVERY_SECS),
        "notification_digest" => Some(DIGEST_SWEEP_EVERY_SECS),
        // Interval is operator-configured; if it was unset since boot, stop.
        "scheduled_backup" => backups::schedule_secs(),
        _ => None,
    };
    if let Some(secs) = cooldown {
        sqlx::query(
            r#"INSERT INTO jobs (id, kind, run_at)
               VALUES ($1, $2, now() + ($3::int || ' seconds')::interval)"#,
        )
        .bind(Uuid::now_v7())
        .bind(kind)
        .bind(secs as i32)
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

/// Send one notification as email, if the recipient still wants it.
///
/// Preferences are read *here*, not at enqueue time: between a comment landing
/// and this job running, someone may have hit unsubscribe. The queue is a
/// delivery mechanism, not a decision record.
async fn run_send_notification_email(
    pool: &PgPool,
    job_id: Uuid,
    mail: &MailCtx,
) -> anyhow::Result<()> {
    let payload: serde_json::Value = sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;
    let Some(nid) = payload
        .get("notification_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Uuid>().ok())
    else {
        // Nothing to send and nothing to retry — a malformed payload won't fix
        // itself, so treat it as done rather than looping ten times.
        warn!(%job_id, "send_notification_email: payload has no notification_id");
        return Ok(());
    };

    let row = sqlx::query!(
        r#"
        SELECT n.kind    AS "kind!: String",
               n.title   AS "title!: String",
               n.body,
               n.link,
               n.user_id AS "user_id!: Uuid",
               u.email   AS "email!: String"
        FROM   notifications n
        JOIN   users u ON u.id = n.user_id
        WHERE  n.id = $1 AND u.deleted_at IS NULL
        "#,
        nid
    )
    .fetch_optional(pool)
    .await?;

    // The notification (or the account) can be gone by now — deleting a task
    // cascades its notifications away. Not an error.
    let Some(row) = row else { return Ok(()) };

    let prefs = email_prefs::load(pool, row.user_id).await?;
    if !prefs.wants_immediate(&row.kind) {
        return Ok(());
    }

    let token = email_prefs::ensure_row(pool, row.user_id).await?;
    let msg = email::notification(
        &mail.public_url,
        &token,
        &row.title,
        row.body.as_deref(),
        row.link.as_deref(),
        &row.email,
    );
    // Awaited, not spawned: a failure here should mark the job failed so the
    // queue's backoff gets its turn, which is the whole point of the outbox.
    mail.mailer.send(msg).await?;
    Ok(())
}

/// Send daily digests to everyone whose local clock has reached their chosen
/// hour and who hasn't had one today.
///
/// Runs every few minutes and does nothing most of the time. Timing is per
/// user, in their own timezone, using the same `AT TIME ZONE` trick the
/// dashboards use — 08:00 should mean 08:00 where they are.
async fn run_notification_digest(pool: &PgPool, mail: &MailCtx) -> anyhow::Result<()> {
    let due = sqlx::query!(
        r#"
        SELECT p.user_id           AS "user_id!: Uuid",
               p.unsubscribe_token AS "token!: Vec<u8>",
               p.kinds             AS "kinds!: serde_json::Value",
               p.last_digest_at,
               u.email             AS "email!: String"
        FROM   email_prefs p
        JOIN   users u ON u.id = p.user_id
        WHERE  p.mode = 'digest'
          AND  u.deleted_at IS NULL
          -- Their local hour has arrived…
          AND  EXTRACT(HOUR FROM (now() AT TIME ZONE COALESCE(u.timezone, 'UTC'))) >= p.digest_hour
          -- …and they haven't had one yet on their local day.
          AND  (p.last_digest_at IS NULL
                OR (p.last_digest_at AT TIME ZONE COALESCE(u.timezone, 'UTC'))::date
                   < (now() AT TIME ZONE COALESCE(u.timezone, 'UTC'))::date)
        LIMIT  500
        "#
    )
    .fetch_all(pool)
    .await?;

    for d in due {
        let since = d
            .last_digest_at
            .unwrap_or_else(|| Utc::now() - chrono::Duration::days(1));
        let items = sqlx::query!(
            r#"
            SELECT kind AS "kind!: String", title AS "title!: String", link
            FROM   notifications
            WHERE  user_id = $1 AND created_at > $2
            ORDER BY created_at
            LIMIT  50
            "#,
            d.user_id,
            since
        )
        .fetch_all(pool)
        .await?;

        // Only the kinds this person left switched on.
        let enabled: Vec<(String, Option<String>)> = items
            .into_iter()
            .filter(|i| {
                d.kinds
                    .get(&i.kind)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            })
            .map(|i| (i.title, i.link))
            .collect();

        // Stamp the send even when there's nothing to say, so a quiet day
        // doesn't queue up tomorrow's digest to cover two days.
        if !enabled.is_empty() {
            let msg = email::digest(&mail.public_url, &d.token, &enabled, &d.email);
            if let Err(e) = mail.mailer.send(msg).await {
                // One bad address shouldn't stop everyone else's digest.
                warn!(error = %e, user_id = %d.user_id, "digest send failed");
                continue;
            }
        }
        sqlx::query("UPDATE email_prefs SET last_digest_at = now() WHERE user_id = $1")
            .bind(d.user_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}
