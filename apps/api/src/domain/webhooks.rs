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
///
/// The job carries the raw `(event, data)`; the worker formats the body per
/// the webhook's `target_type` at delivery time (ADR 0002). A row is
/// deliverable if it's a chat target (URL is the credential) or a generic
/// `outbound` target with a configured signing secret.
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
          AND  $2 = ANY(events)
          AND  (target_type <> 'outbound' OR secret_ciphertext IS NOT NULL)
        "#,
    )
    .bind(project_id)
    .bind(event)
    .fetch_all(pool)
    .await?;

    for id in &ids {
        enqueue(pool, *id, event, &data).await?;
    }
    Ok(ids.len() as u64)
}

/// Enqueue a single delivery job (shared by dispatch + send-test).
async fn enqueue(
    pool: &PgPool,
    webhook_id: Uuid,
    event: &str,
    data: &serde_json::Value,
) -> AppResult<()> {
    sqlx::query(r#"INSERT INTO jobs (id, kind, payload) VALUES ($1, 'deliver_webhook', $2)"#)
        .bind(Uuid::now_v7())
        .bind(json!({ "webhook_id": webhook_id, "event": event, "data": data }))
        .execute(pool)
        .await?;
    Ok(())
}

/// Enqueue a synthetic `test` delivery to one webhook, bypassing the event
/// subscription filter. Lets an admin confirm a target is wired up; the
/// result lands in `webhook_deliveries` like any real delivery.
pub async fn enqueue_test(pool: &PgPool, webhook_id: Uuid) -> AppResult<()> {
    let data = json!({ "key": "TEST-1", "message": "hello from Sprintly" });
    enqueue(pool, webhook_id, "test", &data).await
}
