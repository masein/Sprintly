//! In-app notifications: `@mention` parsing and the `notify` fan-out helper.
//!
//! Notifications are written with runtime queries (no compile-time `.sqlx`
//! cache needed) and pushed live over the existing WS layer via
//! `Event::NotificationCreated`, which the socket delivers only to the target
//! user.

use deadpool_redis::Pool;
use sqlx::PgPool;
use uuid::Uuid;

use crate::infra::events::{publish, Event};
use crate::AppResult;

fn is_handle_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Extract `@handle` mentions from text. Handles are 3–32 chars of
/// `[A-Za-z0-9_]`. An `@` glued to a preceding word char (e.g. inside an email
/// address) is ignored. Results are lowercased and de-duplicated, preserving
/// first-seen order.
pub fn parse_mentions(body: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || !is_handle_char(chars[i - 1])) {
            let mut j = i + 1;
            let mut handle = String::new();
            while j < chars.len() && is_handle_char(chars[j]) {
                handle.push(chars[j]);
                j += 1;
            }
            if (3..=32).contains(&handle.len()) {
                let lower = handle.to_lowercase();
                if !out.contains(&lower) {
                    out.push(lower);
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Resolve mention handles (lowercased) to active users' ids.
pub async fn resolve_handles(db: &PgPool, handles: &[String]) -> AppResult<Vec<Uuid>> {
    if handles.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM users
           WHERE lower(handle) = ANY($1) AND deleted_at IS NULL AND status = 'active'"#,
    )
    .bind(handles)
    .fetch_all(db)
    .await?;
    Ok(ids)
}

/// Create a notification for `recipient` and push a live event. No-op when the
/// recipient is the actor (you don't get notified about your own actions). The
/// DB write is authoritative; the WS push is best-effort.
#[allow(clippy::too_many_arguments)]
pub async fn notify(
    db: &PgPool,
    redis: &Pool,
    recipient: Uuid,
    actor: Uuid,
    kind: &str,
    title: &str,
    body: Option<&str>,
    link: Option<&str>,
    task_id: Option<Uuid>,
) -> AppResult<()> {
    if recipient == actor {
        return Ok(());
    }
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO notifications (id, user_id, actor_id, kind, title, body, link, task_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(id)
    .bind(recipient)
    .bind(actor)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(link)
    .bind(task_id)
    .execute(db)
    .await?;

    publish(
        redis,
        &Event::NotificationCreated {
            user_id: recipient,
            notification_id: id,
        },
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_mentions() {
        assert_eq!(
            parse_mentions("hey @alice and @bob_2"),
            vec!["alice", "bob_2"]
        );
    }

    #[test]
    fn dedupes_case_insensitively_keeps_order() {
        assert_eq!(
            parse_mentions("@Alice @alice @ALICE @carol"),
            vec!["alice", "carol"]
        );
    }

    #[test]
    fn ignores_emails_and_short_handles() {
        // glued to a word char (email) → ignored; "@ab" too short.
        assert_eq!(
            parse_mentions("mail me@example.com or @ab"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn stops_at_punctuation() {
        assert_eq!(parse_mentions("ping @dave, thanks!"), vec!["dave"]);
    }
}
