//! Outbound realtime events. Published to Redis; subscribed to by every
//! `/ws` connection. Topology is intentionally simple for v1:
//!
//!   One channel: "sprintly:events". Every payload carries a `project_id`,
//!   and the WS handler filters by what the user can see. We pay one filter
//!   per message per connection in exchange for never needing to re-subscribe
//!   when membership changes.
//!
//! If/when fan-out gets noisy enough to matter, move to per-project channels:
//!   "sprintly:project:{uuid}". The publisher would then route on
//!   payload.project_id; subscribers would re-subscribe on membership change.

use deadpool_redis::Pool;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

pub const CHANNEL: &str = "sprintly:events";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    TaskCreated { project_id: Uuid, board_id: Uuid, task_id: Uuid, key: String },
    TaskUpdated { project_id: Uuid, task_id: Uuid, key: String },
    TaskMoved {
        project_id: Uuid,
        board_id: Uuid,
        task_id: Uuid,
        key: String,
        from_column_id: Uuid,
        to_column_id: Uuid,
    },
    TaskDeleted { project_id: Uuid, task_id: Uuid, key: String },
    // Reserved for later phases:
    CommentCreated { project_id: Uuid, task_id: Uuid, comment_id: Uuid },
    PresenceUpdate { project_id: Uuid, task_id: Option<Uuid>, user_id: Uuid, active: bool },
    NotificationCreated { user_id: Uuid, notification_id: Uuid },
}

impl Event {
    /// project_id used by the WS filter. `None` = broadcast to all sessions
    /// (used for user-scoped notifications that the WS handler will further
    /// gate on `user_id`).
    pub fn project_scope(&self) -> Option<Uuid> {
        match self {
            Self::TaskCreated { project_id, .. }
            | Self::TaskUpdated { project_id, .. }
            | Self::TaskMoved { project_id, .. }
            | Self::TaskDeleted { project_id, .. }
            | Self::CommentCreated { project_id, .. }
            | Self::PresenceUpdate { project_id, .. } => Some(*project_id),
            Self::NotificationCreated { .. } => None,
        }
    }

    /// User-scope filter, for messages that should only reach one inbox.
    pub fn user_scope(&self) -> Option<Uuid> {
        match self {
            Self::NotificationCreated { user_id, .. } => Some(*user_id),
            _ => None,
        }
    }
}

/// Best-effort publish. If Redis is down we log and return Ok — the DB write
/// already succeeded, and the on-screen state will reconcile on next refetch.
pub async fn publish(redis: &Pool, ev: &Event) {
    let Ok(payload) = serde_json::to_string(ev) else {
        warn!("event serialization failed; dropping");
        return;
    };
    match redis.get().await {
        Ok(mut conn) => {
            if let Err(e) = redis::cmd("PUBLISH")
                .arg(CHANNEL)
                .arg(payload)
                .query_async::<_, i64>(&mut conn)
                .await
            {
                warn!(error = %e, "event publish failed");
            }
        }
        Err(e) => warn!(error = %e, "redis pool failed; event dropped"),
    }
}
