//! `/ws` — the realtime channel.
//!
//! Flow:
//!   1. Upgrade an authenticated GET to a WebSocket.
//!   2. Spawn a Redis subscriber on `sprintly:events`.
//!   3. On each Redis message, filter by the user's accessible projects
//!      (refreshed lazily) and forward as a JSON frame.
//!   4. On each inbound frame (heartbeat or presence hint), respond.
//!
//! Auth happens before the upgrade. The standard browser path is the access
//! cookie; CLI clients can send `Authorization: Bearer ...`.

use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use std::collections::HashSet;
use tokio::time::interval;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    infra::{events::Event, AppState},
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/ws", get(ws_upgrade))
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    user: CurrentUser,
) -> impl IntoResponse {
    let user_id = user.id;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_socket(socket, state, user_id).await {
            warn!(error = %e, "ws session ended with error");
        }
    })
}

async fn handle_socket(socket: WebSocket, state: AppState, user_id: Uuid) -> AppResult<()> {
    let (mut sender, mut receiver) = socket.split();
    info!(%user_id, "ws connected");

    // Initial set of projects the user can see. Refreshed every 30s; also
    // re-evaluated on each event (cheap: HashSet contains is O(1)).
    let mut accessible = load_accessible_projects(&state, user_id).await?;

    // Dedicated Redis pub/sub connection (not from the deadpool — pubsub
    // mode parks the connection).
    let client = redis::Client::open(state.cfg.redis_url.clone())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("redis open: {e}")))?;
    let mut pubsub = client
        .get_async_pubsub()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("redis pubsub: {e}")))?;
    pubsub
        .subscribe(crate::infra::events::CHANNEL)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("redis subscribe: {e}")))?;
    let mut msg_stream = pubsub.on_message();

    let mut refresh = interval(Duration::from_secs(30));
    let mut ping = interval(Duration::from_secs(20));
    // Skip the first tick (fires immediately).
    refresh.tick().await;
    ping.tick().await;

    loop {
        tokio::select! {
            // Redis push → maybe forward.
            Some(msg) = msg_stream.next() => {
                let payload: String = match msg.get_payload() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let event: Event = match serde_json::from_str(&payload) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if !should_forward(&event, user_id, &accessible) {
                    continue;
                }
                if sender.send(Message::Text(payload)).await.is_err() {
                    debug!("ws sender closed; ending");
                    break;
                }
            }

            // Inbound from the client (heartbeats, presence hints, etc.).
            Some(Ok(msg)) = receiver.next() => {
                match msg {
                    Message::Close(_) => break,
                    Message::Ping(p) => {
                        let _ = sender.send(Message::Pong(p)).await;
                    }
                    Message::Pong(_) | Message::Binary(_) => {}
                    Message::Text(t) => {
                        // For now we accept JSON like {"type":"presence","task_id":"...","active":true}
                        // and publish a presence event. Other client messages
                        // are ignored.
                        if let Ok(p) = serde_json::from_str::<ClientPresence>(&t) {
                            let ev = Event::PresenceUpdate {
                                project_id: p.project_id,
                                task_id: p.task_id,
                                user_id,
                                active: p.active,
                            };
                            crate::infra::events::publish(&state.redis, &ev).await;
                        }
                    }
                }
            }

            // Heartbeat ping.
            _ = ping.tick() => {
                if sender.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }

            // Refresh membership snapshot.
            _ = refresh.tick() => {
                accessible = load_accessible_projects(&state, user_id).await.unwrap_or(accessible);
            }
        }
    }

    info!(%user_id, "ws disconnected");
    Ok(())
}

fn should_forward(ev: &Event, user_id: Uuid, accessible: &HashSet<Uuid>) -> bool {
    if let Some(target_user) = ev.user_scope() {
        return target_user == user_id;
    }
    match ev.project_scope() {
        Some(pid) => accessible.contains(&pid),
        None => false,
    }
}

async fn load_accessible_projects(state: &AppState, user_id: Uuid) -> AppResult<HashSet<Uuid>> {
    // Admins see everything; for them we cheaply load all non-deleted projects.
    let is_admin: bool = sqlx::query_scalar(
        r#"SELECT role = 'admin' FROM users WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);

    let ids: Vec<Uuid> = if is_admin {
        sqlx::query_scalar(r#"SELECT id FROM projects WHERE deleted_at IS NULL"#)
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_scalar(
            r#"
            SELECT pm.project_id
            FROM   project_members pm
            JOIN   projects p ON p.id = pm.project_id
            WHERE  pm.user_id = $1 AND p.deleted_at IS NULL
            "#,
        )
        .bind(user_id)
        .fetch_all(&state.db)
        .await?
    };

    Ok(ids.into_iter().collect())
}

#[derive(Debug, serde::Deserialize)]
struct ClientPresence {
    #[serde(rename = "type")]
    _kind: String,
    project_id: Uuid,
    #[serde(default)]
    task_id: Option<Uuid>,
    #[serde(default = "default_true")]
    active: bool,
}
fn default_true() -> bool {
    true
}
