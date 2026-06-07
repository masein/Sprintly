//! Outbound webhook dispatch.
//!
//! `dispatch` finds the active webhooks in a project subscribed to an event and
//! enqueues one `deliver_webhook` job each. The jobs worker signs and POSTs the
//! payload, with the worker's built-in retry/backoff. Best-effort: callers
//! log-and-ignore so dispatch never blocks or fails the request path.

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppResult;

/// Event names a webhook can subscribe to (the `events` array on a webhook).
pub const EVENTS: &[&str] = &[
    "task.created",
    "task.updated",
    "task.moved",
    "task.deleted",
    "comment.created",
];

/// Enqueue a delivery job for each matching webhook. Returns how many.
pub async fn dispatch(
    pool: &PgPool,
    project_id: Uuid,
    event: &str,
    data: serde_json::Value,
) -> AppResult<u64> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id FROM webhooks
        WHERE  project_id = $1 AND active = true AND deleted_at IS NULL
          AND  $2 = ANY(events) AND secret_ciphertext IS NOT NULL
        "#,
    )
    .bind(project_id)
    .bind(event)
    .fetch_all(pool)
    .await?;

    // The signed body is identical for every subscriber.
    let body = json!({ "event": event, "data": data }).to_string();
    for id in &ids {
        sqlx::query(r#"INSERT INTO jobs (id, kind, payload) VALUES ($1, 'deliver_webhook', $2)"#)
            .bind(Uuid::now_v7())
            .bind(json!({ "webhook_id": id, "event": event, "body": body }))
            .execute(pool)
            .await?;
    }
    Ok(ids.len() as u64)
}
