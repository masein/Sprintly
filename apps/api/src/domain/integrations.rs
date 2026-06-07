//! Git provider integration helpers: task-key extraction and webhook signature
//! verification. Pure logic — no DB or HTTP.

use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppResult;

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
pub async fn link(
    db: &PgPool,
    task_key: &str,
    kind: &str,
    ext_ref: &str,
    url: Option<&str>,
    title: Option<&str>,
    pr_state: Option<&str>,
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
        INSERT INTO git_links (id, task_id, provider, kind, external_ref, url, title, state)
        VALUES ($1, $2, 'github', $3, $4, $5, $6, $7)
        ON CONFLICT (task_id, provider, kind, external_ref) DO UPDATE
            SET state = EXCLUDED.state, url = EXCLUDED.url,
                title = EXCLUDED.title, updated_at = now()
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
