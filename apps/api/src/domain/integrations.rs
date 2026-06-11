//! Git provider integration: task-key extraction, webhook signature
//! verification, task↔git linking, and per-project provider connections
//! (ADR 0001).

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Serialize;
use serde_json::json;
use sha2::Sha256;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    domain::vault::{decrypt, encrypt, ProjectKey},
    AppError, AppResult,
};

/// git_integrations secrets are bound to key version 1, same as webhook
/// signing secrets (see `run_deliver_webhook`).
const KEY_VERSION: i32 = 1;

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Extract Sprintly task keys (e.g. `DEMO-1`, `WEB2-42`) from free text such as
/// a commit message or PR title. A key is `[A-Z][A-Z0-9]{1,9}-[0-9]+` at a word
/// boundary. Results are de-duplicated, first-seen order preserved.
pub fn parse_task_keys(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        let boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if boundary && chars[i].is_ascii_uppercase() {
            // Project-key prefix: 2..=10 chars of [A-Z][A-Z0-9]*.
            let mut j = i + 1;
            while j < n
                && (j - i) < 10
                && (chars[j].is_ascii_uppercase() || chars[j].is_ascii_digit())
            {
                j += 1;
            }
            let prefix_len = j - i;
            if (2..=10).contains(&prefix_len) && j < n && chars[j] == '-' {
                let mut k = j + 1;
                while k < n && chars[k].is_ascii_digit() {
                    k += 1;
                }
                let has_digits = k > j + 1;
                let after_ok = k == n || !chars[k].is_ascii_alphanumeric();
                if has_digits && after_ok {
                    let key: String = chars[i..k].iter().collect();
                    if !out.contains(&key) {
                        out.push(key);
                    }
                    i = k;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// Verify a GitHub `X-Hub-Signature-256` header (`sha256=<hex>`) against the raw
/// request body using the shared secret. Constant-time comparison.
pub fn verify_github_signature(secret: &str, body: &[u8], header: &str) -> bool {
    let Some(hex) = header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let expected = hex_encode(&mac.finalize().into_bytes());
    if expected.len() != hex.len() {
        return false;
    }
    use subtle::ConstantTimeEq;
    expected.as_bytes().ct_eq(hex.as_bytes()).into()
}

/// Resolve a task by key and upsert a git link to it, dropping an activity-feed
/// entry when warranted. Returns whether a link was newly created. Unresolved
/// task keys are silently skipped (return `false`).
#[allow(clippy::too_many_arguments)]
pub async fn link(
    db: &PgPool,
    task_key: &str,
    kind: &str,
    ext_ref: &str,
    url: Option<&str>,
    title: Option<&str>,
    pr_state: Option<&str>,
    sha: Option<&str>,
) -> AppResult<bool> {
    let task_id: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT id FROM tasks WHERE key = $1 AND deleted_at IS NULL"#)
            .bind(task_key)
            .fetch_optional(db)
            .await?;
    let Some(task_id) = task_id else {
        return Ok(false);
    };

    // `xmax = 0` is true when the row was inserted (not updated by ON CONFLICT).
    let inserted: bool = sqlx::query_scalar(
        r#"
        INSERT INTO git_links (id, task_id, provider, kind, external_ref, url, title, state, sha)
        VALUES ($1, $2, 'github', $3, $4, $5, $6, $7, $8)
        ON CONFLICT (task_id, provider, kind, external_ref) DO UPDATE
            SET state = EXCLUDED.state, url = EXCLUDED.url,
                title = EXCLUDED.title, sha = COALESCE(EXCLUDED.sha, git_links.sha),
                updated_at = now()
        RETURNING (xmax = 0)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(task_id)
    .bind(kind)
    .bind(ext_ref)
    .bind(url)
    .bind(title)
    .bind(pr_state)
    .bind(sha)
    .fetch_one(db)
    .await?;

    // Announce a merge whenever it happens, and a link the first time it's seen;
    // repeated open/synchronize events stay quiet.
    let activity_kind = if pr_state == Some("merged") {
        Some("pr_merged")
    } else if inserted && kind == "commit" {
        Some("commit_linked")
    } else if inserted {
        Some("pr_linked")
    } else {
        None
    };
    if let Some(ak) = activity_kind {
        sqlx::query(
            r#"INSERT INTO task_activity (id, task_id, actor_id, kind, payload)
               VALUES ($1, $2, NULL, $3, $4)"#,
        )
        .bind(Uuid::now_v7())
        .bind(task_id)
        .bind(ak)
        .bind(json!({ "ref": ext_ref, "url": url, "title": title }))
        .execute(db)
        .await?;
    }

    // A merged PR moves its task to the board's done column.
    if pr_state == Some("merged") {
        transition_to_done(db, task_id).await?;
    }

    // Reflect the (possibly new) task state on the provider side.
    queue_status_updates(db, task_id).await?;

    Ok(inserted)
}

/// Move a task into its board's `done` column (status = done), appended to the
/// bottom. No-op if the board has no done column or the task is already there.
async fn transition_to_done(db: &PgPool, task_id: Uuid) -> AppResult<()> {
    let done_col: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT bc.id
        FROM   board_columns bc
        JOIN   tasks t ON t.board_id = bc.board_id
        WHERE  t.id = $1 AND bc.category = 'done' AND bc.deleted_at IS NULL
        ORDER  BY bc.sort_order ASC
        LIMIT  1
        "#,
    )
    .bind(task_id)
    .fetch_optional(db)
    .await?;
    let Some(col) = done_col else {
        return Ok(());
    };

    let moved = sqlx::query(
        r#"
        UPDATE tasks
        SET    column_id = $2,
               status = 'done',
               completed_at = COALESCE(completed_at, now()),
               order_in_column = COALESCE(
                   (SELECT MAX(order_in_column) + 1024
                    FROM tasks WHERE column_id = $2 AND deleted_at IS NULL),
                   1024)
        WHERE  id = $1 AND (status <> 'done' OR column_id <> $2)
        "#,
    )
    .bind(task_id)
    .bind(col)
    .execute(db)
    .await?;

    if moved.rows_affected() > 0 {
        sqlx::query(
            r#"INSERT INTO task_activity (id, task_id, actor_id, kind, payload)
               VALUES ($1, $2, NULL, 'completed', '{"via":"pr_merge"}'::jsonb)"#,
        )
        .bind(Uuid::now_v7())
        .bind(task_id)
        .execute(db)
        .await?;
    }
    Ok(())
}

// ─── per-project provider connections (ADR 0001) ────────────────────────────

/// Public shape of a connection — secrets stay out; callers learn only
/// whether they're set.
#[derive(Debug, Serialize, FromRow)]
pub struct GitIntegration {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider: String,
    pub repo: String,
    pub base_url: Option<String>,
    pub status_enabled: bool,
    pub has_webhook_secret: bool,
    pub has_api_token: bool,
    pub created_at: DateTime<Utc>,
}

const INTEGRATION_COLS: &str = r#"
    id, project_id, provider, repo, base_url, status_enabled,
    (webhook_secret_ct IS NOT NULL) AS has_webhook_secret,
    (api_token_ct IS NOT NULL) AS has_api_token,
    created_at
"#;

pub async fn list_integrations(db: &PgPool, project_id: Uuid) -> AppResult<Vec<GitIntegration>> {
    let rows = sqlx::query_as(&format!(
        "SELECT {INTEGRATION_COLS} FROM git_integrations WHERE project_id = $1 ORDER BY created_at"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// Create a connection. Secrets are encrypted with the project key
/// (AAD = integration id) before they touch the table.
#[allow(clippy::too_many_arguments)]
pub async fn create_integration(
    db: &PgPool,
    master_key: &[u8; 32],
    project_id: Uuid,
    provider: &str,
    repo: &str,
    base_url: Option<&str>,
    webhook_secret: Option<&str>,
    api_token: Option<&str>,
    status_enabled: bool,
    created_by: Option<Uuid>,
) -> AppResult<GitIntegration> {
    let id = Uuid::now_v7();
    let key = ProjectKey::derive(master_key, project_id, KEY_VERSION);
    let ws = webhook_secret
        .map(|s| encrypt(&key, s.as_bytes(), id.as_bytes()))
        .transpose()?;
    let at = api_token
        .map(|s| encrypt(&key, s.as_bytes(), id.as_bytes()))
        .transpose()?;

    let row = sqlx::query_as(&format!(
        r#"
        INSERT INTO git_integrations
            (id, project_id, provider, repo, base_url,
             webhook_secret_ct, webhook_secret_nonce, api_token_ct, api_token_nonce,
             status_enabled, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING {INTEGRATION_COLS}
        "#
    ))
    .bind(id)
    .bind(project_id)
    .bind(provider)
    .bind(repo)
    .bind(base_url)
    .bind(ws.as_ref().map(|(ct, _)| ct.as_slice()))
    .bind(ws.as_ref().map(|(_, n)| n.as_slice()))
    .bind(at.as_ref().map(|(ct, _)| ct.as_slice()))
    .bind(at.as_ref().map(|(_, n)| n.as_slice()))
    .bind(status_enabled)
    .bind(created_by)
    .fetch_one(db)
    .await
    .map_err(|e| {
        if matches!(&e, sqlx::Error::Database(db) if db.is_unique_violation()) {
            AppError::Conflict("that repo is already connected to this project".into())
        } else {
            e.into()
        }
    })?;
    Ok(row)
}

pub async fn delete_integration(db: &PgPool, id: Uuid, project_id: Uuid) -> AppResult<()> {
    let r = sqlx::query(r#"DELETE FROM git_integrations WHERE id = $1 AND project_id = $2"#)
        .bind(id)
        .bind(project_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Decrypt the API token of an integration. `None` if no token is stored.
pub async fn decrypt_api_token(
    db: &PgPool,
    master_key: &[u8; 32],
    integration_id: Uuid,
) -> AppResult<Option<String>> {
    // (project_id, api_token_ct, api_token_nonce)
    type TokenRow = (Uuid, Option<Vec<u8>>, Option<Vec<u8>>);
    let row: Option<TokenRow> = sqlx::query_as(
        r#"SELECT project_id, api_token_ct, api_token_nonce
           FROM git_integrations WHERE id = $1"#,
    )
    .bind(integration_id)
    .fetch_optional(db)
    .await?;
    let Some((project_id, Some(ct), Some(nonce))) = row else {
        return Ok(None);
    };
    let key = ProjectKey::derive(master_key, project_id, KEY_VERSION);
    let plain = decrypt(&key, &ct, &nonce, integration_id.as_bytes())?;
    Ok(Some(String::from_utf8(plain).map_err(|_| {
        AppError::Crypto("api token is not valid utf-8")
    })?))
}

/// Enqueue one `push_commit_status` job for the task if its project has a
/// status-enabled integration and the task has linked SHAs. The job is
/// self-contained (payload = task id); the runner reads current state, so
/// rapid successive moves collapse into "whatever is true at run time".
pub async fn queue_status_updates(db: &PgPool, task_id: Uuid) -> AppResult<()> {
    let eligible: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM   tasks t
            JOIN   git_integrations gi
                   ON gi.project_id = t.project_id AND gi.status_enabled
                      AND gi.api_token_ct IS NOT NULL
            JOIN   git_links gl ON gl.task_id = t.id AND gl.sha IS NOT NULL
            WHERE  t.id = $1 AND t.deleted_at IS NULL
        )
        "#,
    )
    .bind(task_id)
    .fetch_one(db)
    .await?;
    if !eligible {
        return Ok(());
    }
    sqlx::query(r#"INSERT INTO jobs (id, kind, payload) VALUES ($1, 'push_commit_status', $2)"#)
        .bind(Uuid::now_v7())
        .bind(json!({ "task_id": task_id }))
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_keys_at_boundaries() {
        assert_eq!(
            parse_task_keys("Fix DEMO-1 and WEB2-42 in this PR"),
            vec!["DEMO-1", "WEB2-42"]
        );
    }

    #[test]
    fn dedupes_and_ignores_noise() {
        // lowercase, glued, and over-long prefixes don't match.
        assert_eq!(
            parse_task_keys("DEMO-1 demo-2 xDEMO-3 DEMO-1 TOOLONGPREFIXX-9"),
            vec!["DEMO-1"]
        );
    }

    #[test]
    fn needs_digits_after_dash() {
        assert_eq!(
            parse_task_keys("not a key: ABC- or ABC-x"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn signature_round_trips() {
        let secret = "s3cr3t";
        let body = br#"{"hello":"world"}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let header = format!("sha256={}", hex_encode(&mac.finalize().into_bytes()));
        assert!(verify_github_signature(secret, body, &header));
        assert!(!verify_github_signature("wrong", body, &header));
        assert!(!verify_github_signature(secret, b"tampered", &header));
        assert!(!verify_github_signature(secret, body, "nope"));
    }
}
