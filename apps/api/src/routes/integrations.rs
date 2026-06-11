//! Git provider integration routes (ADR 0001).
//!
//!   GET    /tasks/:task_key/git-links                — linked commits/PRs/branches
//!   GET    /projects/:key/integrations               — list connections (lead)
//!   POST   /projects/:key/integrations               — connect a repo; returns the
//!                                                       webhook secret exactly once
//!   PATCH  /integrations/:id                         — api_token / status_enabled
//!   DELETE /integrations/:id                         — disconnect
//!   POST   /integrations/:provider/webhook/:id       — per-connection inbound
//!   POST   /integrations/github/webhook              — legacy global-secret inbound
//!
//! Inbound webhook routes authenticate via provider signatures (CSRF-exempt);
//! per-connection routes scope task linking to the connection's project.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    domain::{
        git_providers::Provider,
        integrations,
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/integrations/github/webhook", post(github_webhook_legacy))
        .route(
            "/integrations/:provider/webhook/:id",
            post(integration_webhook),
        )
        .route(
            "/projects/:key/integrations",
            get(list_integrations).post(create_integration),
        )
        .route(
            "/integrations/:id",
            axum::routing::patch(update_integration).delete(delete_integration),
        )
        .route("/tasks/:task_key/git-links", get(list_git_links))
}

#[derive(Debug, Serialize, FromRow)]
pub struct GitLinkDto {
    pub id: Uuid,
    pub kind: String,
    pub external_ref: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub state: Option<String>,
    pub created_at: DateTime<Utc>,
}

async fn list_git_links(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let row: Option<(Uuid, Uuid)> =
        sqlx::query_as(r#"SELECT id, project_id FROM tasks WHERE key = $1 AND deleted_at IS NULL"#)
            .bind(&task_key)
            .fetch_optional(&state.db)
            .await?;
    let Some((task_id, project_id)) = row else {
        return Err(AppError::NotFound);
    };
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewBoard, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let links: Vec<GitLinkDto> = sqlx::query_as(
        r#"SELECT id, kind, external_ref, url, title, state, created_at
           FROM git_links WHERE task_id = $1 ORDER BY created_at DESC"#,
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(links))
}

// ─── connection management ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateIntegrationReq {
    provider: String,
    repo: String,
    base_url: Option<String>,
    api_token: Option<String>,
    status_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateIntegrationReq {
    /// `null` clears the stored token; omitted leaves it untouched.
    #[serde(default, with = "double_option")]
    api_token: Option<Option<String>>,
    status_enabled: Option<bool>,
}

/// Distinguish "field absent" from "field: null" for PATCH semantics.
mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Some(Option::<String>::deserialize(d)?))
    }
}

async fn list_integrations(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(
        integrations::list_integrations(&state.db, ctx.id).await?,
    ))
}

async fn create_integration(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateIntegrationReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if Provider::parse(&req.provider).is_none() {
        return Err(AppError::BadRequest(
            "provider must be github, gitlab, or gitea".into(),
        ));
    }
    let repo = req.repo.trim();
    if repo.is_empty() || repo.len() > 200 {
        return Err(AppError::BadRequest("repo must be 1–200 chars".into()));
    }
    let base_url = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(b) = base_url {
        if !b.starts_with("https://") && !b.starts_with("http://") {
            return Err(AppError::BadRequest(
                "base_url must start with http(s)://".into(),
            ));
        }
    }

    // The secret is server-minted and returned exactly once — paste it into
    // the provider's webhook form.
    let webhook_secret = integrations::mint_webhook_secret();
    let integration = integrations::create_integration(
        &state.db,
        &state.cfg.vault.master_key,
        ctx.id,
        &req.provider,
        repo,
        base_url,
        Some(&webhook_secret),
        req.api_token.as_deref().filter(|s| !s.is_empty()),
        req.status_enabled.unwrap_or(false),
        Some(user.id),
    )
    .await?;

    let webhook_path = format!(
        "/api/v1/integrations/{}/webhook/{}",
        integration.provider, integration.id
    );
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "integration": integration,
            "webhook_secret": webhook_secret,
            "webhook_path": webhook_path,
        })),
    ))
}

async fn update_integration(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateIntegrationReq>,
) -> AppResult<impl IntoResponse> {
    let project_id: Uuid =
        sqlx::query_scalar(r#"SELECT project_id FROM git_integrations WHERE id = $1"#)
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let api_token = req.api_token.map(|t| t.filter(|s| !s.trim().is_empty()));
    let updated = integrations::update_integration(
        &state.db,
        &state.cfg.vault.master_key,
        id,
        project_id,
        api_token.as_ref().map(|t| t.as_deref()),
        req.status_enabled,
    )
    .await?;
    Ok(Json(updated))
}

async fn delete_integration(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let project_id: Uuid =
        sqlx::query_scalar(r#"SELECT project_id FROM git_integrations WHERE id = $1"#)
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
    let ctx = project_ctx::load_by_id(&state.db, project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    integrations::delete_integration(&state.db, id, project_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── inbound webhooks ───────────────────────────────────────────────────────

/// Per-connection inbound webhook. The connection's decrypted secret
/// verifies the request, and linking is scoped to the connection's project.
async fn integration_webhook(
    State(state): State<AppState>,
    Path((provider, id)): Path<(String, Uuid)>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<impl IntoResponse> {
    let Some(provider) = Provider::parse(&provider) else {
        return Err(AppError::NotFound);
    };
    // Wrong-provider URLs 404 like unknown ids — don't leak which exist.
    let stored: Option<String> =
        sqlx::query_scalar(r#"SELECT provider FROM git_integrations WHERE id = $1"#)
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    if stored.as_deref() != Some(provider.as_str()) {
        return Err(AppError::NotFound);
    }
    let (project_id, secret) =
        integrations::decrypt_webhook_secret(&state.db, &state.cfg.vault.master_key, id).await?;
    let secret = secret.ok_or(AppError::Unauthorized)?;

    match provider {
        Provider::Github => {
            let sig = headers
                .get("x-hub-signature-256")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !integrations::verify_github_signature(&secret, &body, sig) {
                return Err(AppError::Unauthorized);
            }
            let event = headers
                .get("x-github-event")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let payload: Value = serde_json::from_slice(&body)
                .map_err(|_| AppError::BadRequest("invalid JSON body".into()))?;
            let linked = dispatch_github_event(&state, Some(project_id), event, &payload).await?;
            Ok((StatusCode::OK, Json(json!({ "linked": linked }))))
        }
        // GitLab/Gitea inbound land with the multi-provider slice.
        _ => Err(AppError::BadRequest(
            "inbound webhooks for this provider aren't wired up yet".into(),
        )),
    }
}

/// Legacy global-secret route. Disabled unless `SPRINTLY_GITHUB_WEBHOOK_SECRET`
/// is set; links across all projects (pre-ADR behaviour, kept for installs
/// configured before per-project connections existed).
async fn github_webhook_legacy(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<impl IntoResponse> {
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

    let linked = dispatch_github_event(&state, None, event, &payload).await?;
    Ok((StatusCode::OK, Json(json!({ "linked": linked }))))
}

async fn dispatch_github_event(
    state: &AppState,
    scope: Option<Uuid>,
    event: &str,
    payload: &Value,
) -> AppResult<usize> {
    Ok(match event {
        "ping" => 0,
        "push" => handle_push(state, scope, payload).await?,
        "pull_request" => handle_pull_request(state, scope, payload).await?,
        _ => 0, // event we don't act on
    })
}

async fn handle_push(state: &AppState, scope: Option<Uuid>, payload: &Value) -> AppResult<usize> {
    let mut linked = 0;
    let commits = payload.get("commits").and_then(|c| c.as_array());
    for commit in commits.into_iter().flatten() {
        let message = commit.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let id = commit.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let url = commit.get("url").and_then(|v| v.as_str());
        let short = id.get(..7).unwrap_or(id);
        let title = message.lines().next().unwrap_or("");
        for key in integrations::parse_task_keys(message) {
            if integrations::link(
                &state.db,
                scope,
                &key,
                "commit",
                short,
                url,
                Some(title),
                None,
                Some(id),
            )
            .await?
            {
                linked += 1;
            }
        }
    }
    Ok(linked)
}

async fn handle_pull_request(
    state: &AppState,
    scope: Option<Uuid>,
    payload: &Value,
) -> AppResult<usize> {
    let pr = payload.get("pull_request").cloned().unwrap_or(Value::Null);
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let number = pr.get("number").and_then(|v| v.as_u64()).unwrap_or(0);
    let title = pr.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let pr_body = pr.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let url = pr.get("html_url").and_then(|v| v.as_str());
    let merged = pr.get("merged").and_then(|v| v.as_bool()).unwrap_or(false);
    let head_sha = pr
        .get("head")
        .and_then(|h| h.get("sha"))
        .and_then(|v| v.as_str());

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
            scope,
            &key,
            "pull_request",
            &ext_ref,
            url,
            Some(title),
            Some(state_str),
            head_sha,
        )
        .await?
        {
            linked += 1;
        }
    }
    Ok(linked)
}
