//! Health probes.
//!
//!   GET /healthz  — liveness. Always 200 if the process is up.
//!   GET /readyz   — readiness. 200 only if DB + Redis are reachable.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

use crate::infra::AppState;

pub async fn liveness() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "alive" })))
}

pub async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    let redis_ok = match state.redis.get().await {
        Ok(mut conn) => redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .map(|s| s == "PONG")
            .unwrap_or(false),
        Err(_) => false,
    };

    let ok = db_ok && redis_ok;
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(json!({
            "status": if ok { "ready" } else { "not_ready" },
            "checks": {
                "db": db_ok,
                "redis": redis_ok,
            }
        })),
    )
}
