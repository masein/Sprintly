//! Inbound Git provider webhooks.
//!
//!   POST /integrations/github/webhook
//!
//! GitHub posts `push` / `pull_request` events; we verify the HMAC signature,
//! pull task keys (e.g. `DEMO-1`) out of commit messages and PR titles, and
//! link them to tasks with an activity-feed entry. Unauthenticated — GitHub
//! authenticates via `X-Hub-Signature-256`, so the path is CSRF-exempt.

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};

use crate::{domain::integrations, infra::AppState, AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new().route("/integrations/github/webhook", post(github_webhook))
}

async fn github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<impl IntoResponse> {
    // Disabled unless a secret is configured — behave as if the route is absent.
    let secret = state
        .cfg
        .github_webhook_secret
        .as_deref()
        .ok_or(AppError::NotFound)?;

    let sig = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !integrations::verify_github_signature(secret, &body, sig) {
        return Err(AppError::Unauthorized);
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| AppError::BadRequest("invalid JSON body".into()))?;

    let linked = match event {
        "ping" => 0,
        "push" => handle_push(&state, &payload).await?,
        "pull_request" => handle_pull_request(&state, &payload).await?,
        _ => 0, // event we don't act on
    };

    Ok((StatusCode::OK, Json(json!({ "linked": linked }))))
}

async fn handle_push(state: &AppState, payload: &Value) -> AppResult<usize> {
    let mut linked = 0;
    let commits = payload.get("commits").and_then(|c| c.as_array());
    for commit in commits.into_iter().flatten() {
        let message = commit.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let id = commit.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let url = commit.get("url").and_then(|v| v.as_str());
        let short = id.get(..7).unwrap_or(id);
        let title = message.lines().next().unwrap_or("");
        for key in integrations::parse_task_keys(message) {
            if integrations::link(&state.db, &key, "commit", short, url, Some(title), None).await? {
                linked += 1;
            }
        }
    }
    Ok(linked)
}

async fn handle_pull_request(state: &AppState, payload: &Value) -> AppResult<usize> {
    let pr = payload.get("pull_request").cloned().unwrap_or(Value::Null);
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let number = pr.get("number").and_then(|v| v.as_u64()).unwrap_or(0);
    let title = pr.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let pr_body = pr.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let url = pr.get("html_url").and_then(|v| v.as_str());
    let merged = pr.get("merged").and_then(|v| v.as_bool()).unwrap_or(false);

    let state_str = if merged {
        "merged"
    } else if action == "closed" {
        "closed"
    } else {
        "open"
    };
    let ext_ref = format!("#{number}");
    let text = format!("{title} {pr_body}");

    let mut linked = 0;
    for key in integrations::parse_task_keys(&text) {
        if integrations::link(
            &state.db,
            &key,
            "pull_request",
            &ext_ref,
            url,
            Some(title),
            Some(state_str),
        )
        .await?
        {
            linked += 1;
        }
    }
    Ok(linked)
}
