//! HTTP-level integration tests: these drive the *real* axum router end-to-end
//! (middleware + handlers + serialization + error mapping), which the domain
//! tests don't touch. Auth is via the `Authorization: Bearer` header, which the
//! CSRF guard lets through, so no cookie/CSRF dance is needed.

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sprintly_api::{
    config::{AuthConfig, Config, EmailConfig, Environment, MinioConfig, VaultConfig},
    infra::{email, redis_pool, AppState},
};
use sqlx::PgPool;
use tower::ServiceExt;

fn test_config() -> Config {
    Config {
        env: Environment::Dev,
        public_url: "http://localhost:8080".into(),
        api_bind: "127.0.0.1:8081".parse().unwrap(),
        open_signup: true,
        require_2fa: false,
        local_login_disabled: false,
        oidc: None,
        database_url: String::new(), // unused — the pool is passed in directly
        redis_url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379/0".into()),
        minio: MinioConfig {
            endpoint: "http://localhost:9000".into(),
            public_endpoint: "http://localhost:9000".into(),
            access_key: "sprintly".into(),
            secret_key: "sprintly".into(),
            bucket: "sprintly".into(),
            region: "us-east-1".into(),
        },
        auth: AuthConfig {
            jwt_secret: b"a-test-secret-that-is-long-enough-to-be-fine".to_vec(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
            argon2_m_cost_kib: 4096,
            argon2_t_cost: 1,
            argon2_p_cost: 1,
        },
        vault: VaultConfig {
            master_key: [0u8; 32],
            key_version: 1,
        },
        email: EmailConfig {
            smtp_url: None, // log-only mailer
            mail_from: "Sprintly <noreply@sprintly.test>".into(),
        },
        github_webhook_secret: None,
    }
}

fn app(pool: PgPool) -> Router {
    let cfg = test_config();
    let redis = redis_pool::connect(&cfg).expect("redis pool");
    let mailer = email::build(&cfg.email);
    let state = AppState {
        cfg: Arc::new(cfg),
        db: pool,
        redis,
        mailer,
    };
    sprintly_api::app::router(state)
}

/// Drive one request through the router and return (status, parsed-json body).
async fn send(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let mut req = match body {
        Some(j) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(j.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    // Some handlers (login, rate-limited routes) extract ConnectInfo — the real
    // server injects it; for `oneshot` we add a loopback peer ourselves.
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            8080,
        ))));
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// Register a fresh user and return its access token + the user object.
async fn register(app: &Router, handle: &str) -> (String, Value) {
    let (status, body) = send(
        app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({
            "email": format!("{handle}@sprintly.test"),
            "handle": handle,
            "display_name": "Test User",
            "password": "correct-horse-battery-staple",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register failed: {body:?}");
    let token = body["access_token"]
        .as_str()
        .expect("access_token")
        .to_string();
    (token, body["user"].clone())
}

/// Create a project and return its key.
async fn make_project(app: &Router, token: &str, key: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/api/v1/projects",
        Some(token),
        Some(json!({ "key": key, "name": format!("{key} Project") })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "create project failed: {body:?}"
    );
    body["key"].as_str().unwrap().to_string()
}

/// The default board's columns as (id, category) pairs.
async fn columns(app: &Router, token: &str, key: &str) -> Vec<(String, String)> {
    let (status, body) = send(
        app,
        "GET",
        &format!("/api/v1/projects/{key}/boards"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["items"][0]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap().to_string(),
                c["category"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn register_returns_a_session_and_me_requires_auth(pool: PgPool) {
    let app = app(pool);
    let (token, user) = register(&app, "admin1").await;
    // The first user is bootstrapped as an admin.
    assert_eq!(user["role"], "admin");

    // The token resolves the current user via the CurrentUser extractor.
    let (status, me) = send(&app, "GET", "/api/v1/users/me", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], "admin1@sprintly.test");
    assert_eq!(me["handle"], "admin1");

    // No token → 401 from the auth middleware.
    let (status, _) = send(&app, "GET", "/api/v1/users/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn must_change_password_forces_reset_at_login(pool: PgPool) {
    // A provisioned-style account: real password hash + the force-reset flag.
    let hash = sprintly_api::domain::password::hash(&test_config().auth, "123456").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, must_change_password)
           VALUES ($1, 'reset@x.test', 'resetme', 'Reset Me', $2, 'member', true)"#,
    )
    .bind(uuid::Uuid::now_v7())
    .bind(&hash)
    .execute(&pool)
    .await
    .unwrap();
    let app = app(pool);

    // Login with the temp password → a force-reset challenge, NOT a session.
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({ "email": "reset@x.test", "password": "123456" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["must_change_password_required"], true);
    assert!(body["access_token"].is_null(), "no session yet");
    let challenge = body["challenge"].as_str().unwrap().to_string();

    // Spend the challenge to set a new password → a real session.
    let (status, changed) = send(
        &app,
        "POST",
        "/api/v1/auth/password/change",
        None,
        Some(json!({ "challenge": challenge, "new_password": "a-fresh-strong-pass" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed:?}");
    assert!(changed["access_token"].is_string());

    // Logging in with the new password now succeeds normally (flag cleared).
    let (status, ok) = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({ "email": "reset@x.test", "password": "a-fresh-strong-pass" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        ok["access_token"].is_string(),
        "normal session, no challenge"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn project_lifecycle_over_http(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "lead1").await;

    let key = make_project(&app, &token, "WEB").await;
    assert_eq!(key, "WEB");

    // Detail + list both see it.
    let (status, dto) = send(&app, "GET", "/api/v1/projects/WEB", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dto["key"], "WEB");

    let (status, list) = send(&app, "GET", "/api/v1/projects", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let keys: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().unwrap())
        .collect();
    assert!(keys.contains(&"WEB"));

    // The default board ships with the three standard columns.
    let cols = columns(&app, &token, "WEB").await;
    let cats: Vec<&str> = cols.iter().map(|(_, c)| c.as_str()).collect();
    assert!(cats.contains(&"todo") && cats.contains(&"in_progress") && cats.contains(&"done"));
}

#[sqlx::test(migrations = "./migrations")]
async fn task_crud_and_move_over_http(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "dev1").await;
    make_project(&app, &token, "API").await;
    let cols = columns(&app, &token, "API").await;
    let in_progress = cols
        .iter()
        .find(|(_, c)| c == "in_progress")
        .unwrap()
        .0
        .clone();

    // Create → lands in a todo column.
    let (status, task) = send(
        &app,
        "POST",
        "/api/v1/projects/API/tasks",
        Some(&token),
        Some(json!({ "title": "Ship the thing" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task:?}");
    let task_key = task["key"].as_str().unwrap().to_string();
    assert_eq!(task["status"], "todo");

    // It shows up in the board list.
    let (status, list) = send(
        &app,
        "GET",
        "/api/v1/projects/API/tasks",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // Read it back.
    let (status, got) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{task_key}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["title"], "Ship the thing");

    // Edit priority via PATCH.
    let (status, edited) = send(
        &app,
        "PATCH",
        &format!("/api/v1/tasks/{task_key}"),
        Some(&token),
        Some(json!({ "priority": "p0" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(edited["priority"], "p0");

    // Move it to In progress — status follows the destination column's category.
    let (status, moved) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{task_key}/move"),
        Some(&token),
        Some(json!({ "column_id": in_progress })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved:?}");
    assert_eq!(moved["status"], "in_progress");
}

#[sqlx::test(migrations = "./migrations")]
async fn sprint_lifecycle_over_http(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "scrum1").await;
    make_project(&app, &token, "SPR").await;

    // A task to commit to the sprint.
    let (status, task) = send(
        &app,
        "POST",
        "/api/v1/projects/SPR/tasks",
        Some(&token),
        Some(json!({ "title": "Sprint work" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let task_key = task["key"].as_str().unwrap().to_string();

    // Create a sprint (starts life "planned").
    let (status, sprint) = send(
        &app,
        "POST",
        "/api/v1/projects/SPR/sprints",
        Some(&token),
        Some(json!({
            "name": "Sprint 1",
            "starts_at": "2026-06-19T00:00:00Z",
            "ends_at": "2026-07-03T00:00:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sprint:?}");
    let sprint_id = sprint["id"].as_str().unwrap().to_string();
    assert_eq!(sprint["state"], "planned");

    // Commit the task, then start the sprint.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint_id}/tasks/{task_key}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, started) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint_id}/start"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started:?}");
    assert_eq!(started["state"], "active");

    // The committed task is listed under the sprint.
    let (status, list) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/tasks"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn invalid_input_is_rejected_over_http(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "picky1").await;

    // Lowercase key violates the project key rule.
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/projects",
        Some(&token),
        Some(json!({ "key": "web", "name": "Bad Key" })),
    )
    .await;
    assert!(status.is_client_error(), "expected 4xx, got {status}");

    // Empty title violates the task validation.
    make_project(&app, &token, "OK").await;
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/projects/OK/tasks",
        Some(&token),
        Some(json!({ "title": "" })),
    )
    .await;
    assert!(status.is_client_error(), "expected 4xx, got {status}");
}
