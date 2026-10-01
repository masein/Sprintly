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

/// An explicitly-presented invite must win over open signup: the invitee gets
/// the invite's role and the token is consumed. No token + open signup still
/// lands as member. (Regression: the open-signup branch used to short-circuit
/// and silently ignore the invite entirely.)
#[sqlx::test(migrations = "./migrations")]
async fn invite_role_wins_over_open_signup(pool: PgPool) {
    let app = app(pool.clone());

    // First user is admin by the first-boot rule.
    let (admin_token, _) = register(&app, "founder").await;

    // Admin mints an admin invite.
    let (status, invite) = send(
        &app,
        "POST",
        "/api/v1/admin/invites",
        Some(&admin_token),
        Some(json!({ "suggested_role": "admin" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "mint failed: {invite:?}");
    let token_plain = invite["token"].as_str().unwrap().to_string();
    let invite_id = invite["id"].as_str().unwrap().to_string();

    // Open signup is ON in the test config — the invite must still apply.
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({
            "email": "invited@sprintly.test",
            "handle": "invited",
            "display_name": "Invited Admin",
            "password": "correct-horse-battery-staple",
            "invite_token": token_plain,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "invited register failed: {body:?}");
    assert_eq!(
        body["user"]["role"], "admin",
        "invite role must apply: {body:?}"
    );

    // The token is single-use: consumed_at is stamped.
    let consumed: bool =
        sqlx::query_scalar("SELECT consumed_at IS NOT NULL FROM invite_tokens WHERE id = $1::uuid")
            .bind(&invite_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(consumed, "invite must be marked consumed");

    // No token + open signup → plain member, as before.
    let (_, walkin) = register(&app, "walkin").await;
    assert_eq!(walkin["role"], "member");
}

/// Admin email change over HTTP: applies, rejects duplicates with 409, and
/// requires admin.
#[sqlx::test(migrations = "./migrations")]
async fn admin_can_change_a_user_email(pool: PgPool) {
    let app = app(pool.clone());
    let (admin_token, _) = register(&app, "boss").await; // first user → admin
    let (member_token, member) = register(&app, "worker").await;
    let member_id = member["id"].as_str().unwrap().to_string();

    // Happy path.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{member_id}/email"),
        Some(&admin_token),
        Some(json!({ "email": "worker-new@sprintly.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let stored: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id = $1::uuid")
        .bind(&member_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, "worker-new@sprintly.test");

    // Duplicate (case-variant of the admin's email) → 409.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{member_id}/email"),
        Some(&admin_token),
        Some(json!({ "email": "BOSS@sprintly.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "expected 409: {body:?}");

    // Non-admin is refused.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{member_id}/email"),
        Some(&member_token),
        Some(json!({ "email": "sneaky@sprintly.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ─── Retro note editing (QA-4) ───────────────────────────────────────────────

/// Drive a project to an open retro; returns (sprint_id, retro_id).
async fn open_retro(app: &Router, token: &str, key: &str) -> (String, String) {
    make_project(app, token, key).await;
    let (status, sprint) = send(
        app,
        "POST",
        &format!("/api/v1/projects/{key}/sprints"),
        Some(token),
        Some(json!({
            "name": "Retro Sprint",
            "starts_at": "2026-06-19T00:00:00Z",
            "ends_at": "2026-07-03T00:00:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sprint:?}");
    let sprint_id = sprint["id"].as_str().unwrap().to_string();
    for step in ["start", "complete"] {
        let (status, body) = send(
            app,
            "POST",
            &format!("/api/v1/sprints/{sprint_id}/{step}"),
            Some(token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{step}: {body:?}");
    }
    let (status, retro) = send(
        app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/retro"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{retro:?}");
    assert_eq!(retro["state"], "open");
    let retro_id = retro["id"].as_str().unwrap().to_string();
    (sprint_id, retro_id)
}

async fn fetch_first_note(app: &Router, token: &str, sprint_id: &str) -> Value {
    let (status, retro) = send(
        app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/retro"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    retro["notes"]["went_well"][0].clone()
}

#[sqlx::test(migrations = "./migrations")]
async fn retro_note_editing_rules_over_http(pool: PgPool) {
    let app = app(pool);
    let (lead_token, lead) = register(&app, "retrolead").await;
    let (sprint_id, retro_id) = open_retro(&app, &lead_token, "RNE").await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/retros/{retro_id}/notes"),
        Some(&lead_token),
        Some(json!({ "column_kind": "went_well", "body": "first draft" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The DTO exposes author_id on a non-anonymous note, and no edit flag yet.
    let note = fetch_first_note(&app, &lead_token, &sprint_id).await;
    let note_id = note["id"].as_str().unwrap().to_string();
    assert_eq!(note["author_id"], lead["id"]);
    assert_eq!(note["edited"], false);

    // The author edits their note.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/retro-notes/{note_id}"),
        Some(&lead_token),
        Some(json!({ "body": "second thoughts" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // A non-member can't touch it — membership is checked before ownership.
    let (stranger_token, _) = register(&app, "retrostranger").await;
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/retro-notes/{note_id}"),
        Some(&stranger_token),
        Some(json!({ "body": "vandalism" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The edit landed and is flagged as edited.
    let note = fetch_first_note(&app, &lead_token, &sprint_id).await;
    assert_eq!(note["body"], "second thoughts");
    assert_eq!(note["edited"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn retro_note_editing_locks_with_the_retro(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "retrocloser").await;
    let (sprint_id, retro_id) = open_retro(&app, &token, "RNC").await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/retros/{retro_id}/notes"),
        Some(&token),
        Some(json!({ "column_kind": "went_well", "body": "for the record" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let note = fetch_first_note(&app, &token, &sprint_id).await;
    let note_id = note["id"].as_str().unwrap().to_string();

    // Anonymous notes expose no author_id even to their author.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/retros/{retro_id}/notes"),
        Some(&token),
        Some(json!({ "column_kind": "went_well", "body": "whistleblowing", "anonymous": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, retro) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/retro"),
        Some(&token),
        None,
    )
    .await;
    let anon = retro["notes"]["went_well"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["anonymous"] == true)
        .unwrap()
        .clone();
    assert_eq!(anon["author_id"], Value::Null);

    // Close the retro → edits are refused, the summary already snapshotted.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/retros/{retro_id}/close"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/retro-notes/{note_id}"),
        Some(&token),
        Some(json!({ "body": "revisionism" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

// ─── Retro summary editing (QA-5) ────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn retro_summary_is_editable_after_close(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "sumlead").await;
    make_project(&app, &token, "SUM").await;

    let (status, sprint) = send(
        &app,
        "POST",
        "/api/v1/projects/SUM/sprints",
        Some(&token),
        Some(json!({
            "name": "Summary Sprint",
            "starts_at": "2026-06-19T00:00:00Z",
            "ends_at": "2026-07-03T00:00:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sprint:?}");
    let sprint_id = sprint["id"].as_str().unwrap().to_string();

    // Editing the summary before the sprint completes is refused.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/sprints/{sprint_id}"),
        Some(&token),
        Some(json!({ "summary_md": "premature" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    for step in ["start", "complete"] {
        let (status, body) = send(
            &app,
            "POST",
            &format!("/api/v1/sprints/{sprint_id}/{step}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{step}: {body:?}");
    }
    let (_, retro) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/retro"),
        Some(&token),
        None,
    )
    .await;
    let retro_id = retro["id"].as_str().unwrap();
    let (status, closed) = send(
        &app,
        "POST",
        &format!("/api/v1/retros/{retro_id}/close"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed:?}");

    // The lead refines the generated summary.
    let (status, updated) = send(
        &app,
        "PATCH",
        &format!("/api/v1/sprints/{sprint_id}"),
        Some(&token),
        Some(json!({ "summary_md": "# Reworked\n\nHuman words now." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated:?}");
    assert_eq!(updated["summary_md"], "# Reworked\n\nHuman words now.");

    // Meta edits on a completed sprint stay refused.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/sprints/{sprint_id}"),
        Some(&token),
        Some(json!({ "name": "Rename after the fact" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // A non-member can't touch it.
    let (stranger, _) = register(&app, "sumstranger").await;
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/sprints/{sprint_id}"),
        Some(&stranger),
        Some(json!({ "summary_md": "graffiti" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ─── Task ↔ subtask conversion (QA-9) ────────────────────────────────────────

async fn make_task_http(app: &Router, token: &str, project: &str, title: &str) -> String {
    let (status, task) = send(
        app,
        "POST",
        &format!("/api/v1/projects/{project}/tasks"),
        Some(token),
        Some(json!({ "title": title })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task:?}");
    task["key"].as_str().unwrap().to_string()
}

async fn set_parent(app: &Router, token: &str, key: &str, parent: Option<&str>) -> StatusCode {
    let (status, body) = send(
        app,
        "PUT",
        &format!("/api/v1/tasks/{key}/parent"),
        Some(token),
        Some(json!({ "parent_key": parent })),
    )
    .await;
    assert!(
        !status.is_server_error(),
        "set_parent({key}, {parent:?}) → {status}: {body:?}"
    );
    status
}

async fn board_keys(app: &Router, token: &str, project: &str) -> Vec<String> {
    let (status, list) = send(
        app,
        "GET",
        &format!("/api/v1/projects/{project}/tasks"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["key"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn task_subtask_conversion_over_http(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "converter").await;
    make_project(&app, &token, "CVT").await;
    let a = make_task_http(&app, &token, "CVT", "parent to be").await;
    let b = make_task_http(&app, &token, "CVT", "future subtask").await;
    let c = make_task_http(&app, &token, "CVT", "second parent").await;

    // Demote B under A → B leaves the board list.
    assert_eq!(
        set_parent(&app, &token, &b, Some(&a)).await,
        StatusCode::NO_CONTENT
    );
    let keys = board_keys(&app, &token, "CVT").await;
    assert!(keys.contains(&a) && keys.contains(&c) && !keys.contains(&b));
    let (_, task_b) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{b}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(task_b["parent_key"], a.as_str());

    // Guards: no nesting under a subtask; no demoting a task with children;
    // no being your own parent.
    assert_eq!(
        set_parent(&app, &token, &c, Some(&b)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        set_parent(&app, &token, &a, Some(&c)).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        set_parent(&app, &token, &c, Some(&c)).await,
        StatusCode::BAD_REQUEST
    );

    // Reparent B from A to C.
    assert_eq!(
        set_parent(&app, &token, &b, Some(&c)).await,
        StatusCode::NO_CONTENT
    );
    let (_, task_b) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{b}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(task_b["parent_key"], c.as_str());

    // Cross-project parents are refused.
    make_project(&app, &token, "CVX").await;
    let x = make_task_http(&app, &token, "CVX", "elsewhere").await;
    assert_eq!(
        set_parent(&app, &token, &b, Some(&x)).await,
        StatusCode::BAD_REQUEST
    );

    // Promote B → back on the board.
    assert_eq!(
        set_parent(&app, &token, &b, None).await,
        StatusCode::NO_CONTENT
    );
    let keys = board_keys(&app, &token, "CVT").await;
    assert!(keys.contains(&b));
    let (_, task_b) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{b}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(task_b["parent_key"], Value::Null);
}

#[sqlx::test(migrations = "./migrations")]
async fn demoting_a_sprint_task_clears_its_sprint(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "demoter").await;
    make_project(&app, &token, "DMS").await;
    let a = make_task_http(&app, &token, "DMS", "the parent").await;
    let b = make_task_http(&app, &token, "DMS", "committed then demoted").await;

    let (status, sprint) = send(
        &app,
        "POST",
        "/api/v1/projects/DMS/sprints",
        Some(&token),
        Some(json!({
            "name": "Sprint 1",
            "starts_at": "2026-06-19T00:00:00Z",
            "ends_at": "2026-07-03T00:00:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sprint:?}");
    let sprint_id = sprint["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint_id}/tasks/{b}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Demote → sprint membership drops so the sprint doesn't count it twice.
    assert_eq!(
        set_parent(&app, &token, &b, Some(&a)).await,
        StatusCode::NO_CONTENT
    );
    let (_, task_b) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{b}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(task_b["sprint_id"], Value::Null);
    let (_, list) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/tasks"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
}

// ─── Honest 4xx messages (QA2-3) ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn conflict_responses_carry_their_real_message(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "conflicter").await;
    make_project(&app, &token, "CNF").await;

    // Put a task in the first column, then try to delete that column.
    let (status, task) = send(
        &app,
        "POST",
        "/api/v1/projects/CNF/tasks",
        Some(&token),
        Some(json!({ "title": "squatter" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task:?}");
    let cols = columns(&app, &token, "CNF").await;
    let first_col = &cols[0].0;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/api/v1/columns/{first_col}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    // The point of the fix: the handler's message survives to the client
    // instead of being flattened to "That already exists."
    assert_eq!(
        body["error"]["message"],
        "column still has tasks — move them first"
    );

    // BadRequest messages survive too.
    let (status, body) = send(
        &app,
        "PUT",
        &format!("/api/v1/tasks/{}/parent", task["key"].as_str().unwrap()),
        Some(&token),
        Some(json!({ "parent_key": task["key"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["message"], "a task can't be its own parent");
}

// ─── Project key rename (QA2-9) ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn project_key_rename_cascades_to_task_keys(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "renamer").await;
    make_project(&app, &token, "OLD1").await;
    for title in ["first", "second"] {
        let (status, _) = send(
            &app,
            "POST",
            "/api/v1/projects/OLD1/tasks",
            Some(&token),
            Some(json!({ "title": title })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // Rename the key.
    let (status, project) = send(
        &app,
        "PATCH",
        "/api/v1/projects/OLD1",
        Some(&token),
        Some(json!({ "key": "NEW1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project:?}");
    assert_eq!(project["key"], "NEW1");

    // The project answers at the new key, not the old one.
    let (status, _) = send(&app, "GET", "/api/v1/projects/NEW1", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&app, "GET", "/api/v1/projects/OLD1", Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Task keys were rewritten with it.
    let (status, task) = send(&app, "GET", "/api/v1/tasks/NEW1-1", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{task:?}");
    assert_eq!(task["project_key"], "NEW1");
    let (status, _) = send(&app, "GET", "/api/v1/tasks/OLD1-1", Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The sequence keeps counting — no key reuse.
    let (status, task) = send(
        &app,
        "POST",
        "/api/v1/projects/NEW1/tasks",
        Some(&token),
        Some(json!({ "title": "third" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(task["key"], "NEW1-3");

    // A key another project owns is refused with the real message.
    make_project(&app, &token, "TAKEN").await;
    let (status, body) = send(
        &app,
        "PATCH",
        "/api/v1/projects/NEW1",
        Some(&token),
        Some(json!({ "key": "TAKEN" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"]["message"],
        "that key belongs to another project"
    );

    // Format rules still apply.
    let (status, _) = send(
        &app,
        "PATCH",
        "/api/v1/projects/NEW1",
        Some(&token),
        Some(json!({ "key": "1BAD" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ─── Sprint completion carry-over + delete (QA3-1, QA3-2) ────────────────────

/// Create a sprint and return its id.
async fn make_sprint(app: &Router, token: &str, project: &str, name: &str) -> String {
    let (status, sprint) = send(
        app,
        "POST",
        &format!("/api/v1/projects/{project}/sprints"),
        Some(token),
        Some(json!({
            "name": name,
            "starts_at": "2026-06-19T00:00:00Z",
            "ends_at": "2026-07-03T00:00:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sprint:?}");
    sprint["id"].as_str().unwrap().to_string()
}

async fn sprint_task_keys(app: &Router, token: &str, sprint_id: &str) -> Vec<String> {
    let (status, list) = send(
        app,
        "GET",
        &format!("/api/v1/sprints/{sprint_id}/tasks"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["key"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn completing_a_sprint_carries_unfinished_work(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "carrier").await;
    make_project(&app, &token, "CRY").await;
    let done = make_task_http(&app, &token, "CRY", "finished").await;
    let open = make_task_http(&app, &token, "CRY", "still going").await;
    let sprint = make_sprint(&app, &token, "CRY", "Sprint 1").await;
    let next = make_sprint(&app, &token, "CRY", "Sprint 2").await;

    for key in [&done, &open] {
        let (status, _) = send(
            &app,
            "POST",
            &format!("/api/v1/sprints/{sprint}/tasks/{key}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    // Mark one done — status follows the column, so move it to the done one.
    let done_col = columns(&app, &token, "CRY")
        .await
        .into_iter()
        .find(|(_, cat)| cat == "done")
        .expect("a done column")
        .0;
    let (status, moved) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{done}/move"),
        Some(&token),
        Some(json!({ "column_id": done_col })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved:?}");
    assert_eq!(moved["status"], "done");

    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint}/start"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Complete, carrying the leftovers into Sprint 2.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint}/complete"),
        Some(&token),
        Some(json!({ "carry_over": { "to": "sprint", "sprint_id": next } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["sprint"]["state"], "completed");
    assert_eq!(body["carried_over"], 1);
    assert_eq!(body["carried_to"]["id"], next.as_str());

    // The unfinished one moved: the next sprint has it live…
    assert_eq!(
        sprint_task_keys(&app, &token, &next).await,
        vec![open.clone()]
    );
    // …and the completed sprint still lists *both*, as they stood when it
    // closed. Carrying work forward no longer rewrites the sprint it left
    // (QA report 5's historical-snapshot model).
    let mut history = sprint_task_keys(&app, &token, &sprint).await;
    history.sort();
    let mut expected = vec![done.clone(), open.clone()];
    expected.sort();
    assert_eq!(history, expected, "completed sprint keeps its history");
}

#[sqlx::test(migrations = "./migrations")]
async fn carry_over_to_backlog_and_to_a_new_sprint(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "carrier2").await;
    make_project(&app, &token, "CR2").await;

    // Round 1: carry to the backlog.
    let a = make_task_http(&app, &token, "CR2", "leftover a").await;
    let s1 = make_sprint(&app, &token, "CR2", "Sprint 1").await;
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s1}/tasks/{a}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s1}/start"),
        Some(&token),
        None,
    )
    .await;
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s1}/complete"),
        Some(&token),
        Some(json!({ "carry_over": { "to": "backlog" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["carried_over"], 1);
    assert_eq!(body["carried_to"], Value::Null);
    // History survives the move: the completed sprint still names the task.
    assert_eq!(sprint_task_keys(&app, &token, &s1).await.len(), 1);
    let (_, task) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{a}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(task["sprint_id"], Value::Null);

    // Round 2: carry into a brand-new sprint created on the spot.
    let s2 = make_sprint(&app, &token, "CR2", "Sprint 2").await;
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s2}/tasks/{a}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s2}/start"),
        Some(&token),
        None,
    )
    .await;
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s2}/complete"),
        Some(&token),
        Some(json!({ "carry_over": {
            "to": "new_sprint",
            "name": "Sprint 3",
            "starts_at": "2026-07-06T00:00:00Z",
            "ends_at": "2026-07-20T00:00:00Z",
        }})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["carried_over"], 1);
    assert_eq!(body["carried_to"]["name"], "Sprint 3");
    let fresh = body["carried_to"]["id"].as_str().unwrap();
    assert_eq!(sprint_task_keys(&app, &token, fresh).await, vec![a]);
    // The new sprint starts life planned, not running.
    let (_, s) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{fresh}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(s["state"], "planned");
}

#[sqlx::test(migrations = "./migrations")]
async fn sprint_dates_editable_while_active_and_delete_frees_tasks(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "sprintedit").await;
    make_project(&app, &token, "SED").await;
    let key = make_task_http(&app, &token, "SED", "homeless soon").await;
    let s = make_sprint(&app, &token, "SED", "Sprint 1").await;
    send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s}/tasks/{key}"),
        Some(&token),
        None,
    )
    .await;
    send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{s}/start"),
        Some(&token),
        None,
    )
    .await;

    // Dates move while the sprint is running (used to be planned-only).
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/api/v1/sprints/{s}"),
        Some(&token),
        Some(json!({ "name": "Sprint 1 (extended)", "ends_at": "2026-07-17T00:00:00Z" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["name"], "Sprint 1 (extended)");
    assert!(body["ends_at"].as_str().unwrap().starts_with("2026-07-17"));

    // Delete: the sprint goes, the task returns to the backlog.
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/sprints/{s}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{s}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, task) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{key}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task["sprint_id"], Value::Null);
}

// ─── Settings merge + project appearance (QA3-11/12) ─────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn patch_me_merges_settings_instead_of_replacing(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "settingsmerger").await;

    // Screen A stores its preference.
    let (status, _) = send(
        &app,
        "PATCH",
        "/api/v1/users/me",
        Some(&token),
        Some(json!({ "settings": { "coffee_meter": false } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Screen B stores a different one — without knowing about the first.
    let (status, me) = send(
        &app,
        "PATCH",
        "/api/v1/users/me",
        Some(&token),
        Some(json!({ "settings": { "project_order": ["ONE", "TWO"] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Both survive: the old whole-blob write dropped coffee_meter here.
    assert_eq!(me["settings"]["coffee_meter"], false);
    assert_eq!(me["settings"]["project_order"][1], "TWO");

    // Same key overwrites, as you'd expect.
    let (_, me) = send(
        &app,
        "PATCH",
        "/api/v1/users/me",
        Some(&token),
        Some(json!({ "settings": { "project_order": ["THREE"] } })),
    )
    .await;
    assert_eq!(me["settings"]["project_order"], json!(["THREE"]));
    assert_eq!(me["settings"]["coffee_meter"], false);
}

#[sqlx::test(migrations = "./migrations")]
async fn project_icon_and_color_are_editable(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "repainter").await;
    make_project(&app, &token, "PNT").await;

    let (status, p) = send(
        &app,
        "PATCH",
        "/api/v1/projects/PNT",
        Some(&token),
        Some(json!({ "icon": "rocket", "color": "#22d3ee" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p:?}");
    assert_eq!(p["icon"], "rocket");
    assert_eq!(p["color"], "#22d3ee");

    // Junk colour is refused, and the stored value is untouched.
    let (status, _) = send(
        &app,
        "PATCH",
        "/api/v1/projects/PNT",
        Some(&token),
        Some(json!({ "color": "octarine" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, p) = send(&app, "GET", "/api/v1/projects/PNT", Some(&token), None).await;
    assert_eq!(p["color"], "#22d3ee");
}

// ── JQL search + saved queries (feat/jql-search) ─────────────────────────

async fn jql(app: &Router, token: &str, q: &str) -> (StatusCode, Value) {
    let encoded = q
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('"', "%22")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('+', "%2B")
        .replace('=', "%3D")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace(',', "%2C")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('~', "%7E");
    send(
        app,
        "GET",
        &format!("/api/v1/search/jql?jql={encoded}"),
        Some(token),
        None,
    )
    .await
}

fn keys(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["key"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn jql_filters_tasks_and_never_leaves_your_projects(pool: PgPool) {
    let app = app(pool);
    // The very first account is bootstrapped as an admin, and admins can see
    // every project — so burn one to make alice and bob ordinary members.
    let _ = register(&app, "jqlroot").await;
    let (alice, _) = register(&app, "jqlalice").await;
    let (bob, _) = register(&app, "jqlbob").await;
    make_project(&app, &alice, "JQLA").await;
    make_project(&app, &bob, "JQLB").await;
    let a1 = make_task_http(&app, &alice, "JQLA", "alpha login bug").await;
    let a2 = make_task_http(&app, &alice, "JQLA", "beta export crash").await;
    let b1 = make_task_http(&app, &bob, "JQLB", "alpha login bug").await;

    // An empty query is "everything I can see" — and no more.
    let (status, body) = jql(&app, &alice, "").await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let mine = keys(&body);
    assert!(mine.contains(&a1) && mine.contains(&a2), "{mine:?}");
    assert!(!mine.contains(&b1), "alice saw bob's task: {mine:?}");
    assert_eq!(body["total"], 2);

    // Text match narrows within that scope.
    let (status, body) = jql(&app, &alice, "title ~ alpha").await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(keys(&body), vec![a1.clone()]);

    // Bob's identical title is his own, still invisible to alice.
    let (_, body) = jql(&app, &bob, "title ~ alpha").await;
    assert_eq!(keys(&body), vec![b1]);

    // currentUser() resolves per caller. Nothing is assigned yet, so the
    // negative form is what proves the substitution happened at all.
    let (status, body) = jql(&app, &alice, "assignee is empty").await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["total"], 2);
    let (status, body) = jql(&app, &alice, "assignee = currentUser()").await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["total"], 0);

    // in-lists, ORDER BY, and a field the schema really has.
    let (status, body) = jql(
        &app,
        &alice,
        "status in (todo, in_progress) AND project = JQLA ORDER BY key ASC",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(keys(&body), vec![a1, a2]);
}

#[sqlx::test(migrations = "./migrations")]
async fn jql_syntax_errors_say_where(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "jqlerr").await;
    make_project(&app, &token, "JQLE").await;

    for bad in [
        "banana = 3",
        "status todo",
        "(status = todo",
        "points = high",
    ] {
        let (status, body) = jql(&app, &token, bad).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "`{bad}` should be rejected: {body:?}"
        );
        let msg = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            msg.contains("character"),
            "`{bad}` should point at a position, got: {msg}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn saved_queries_round_trip(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "jqlsave").await;

    let (status, made) = send(
        &app,
        "POST",
        "/api/v1/search/queries",
        Some(&token),
        Some(
            json!({ "name": "My open work", "jql": "assignee = currentUser() AND status != done" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made:?}");
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(made["is_mine"], true);
    assert_eq!(made["is_shared"], false);

    let (status, body) = send(&app, "GET", "/api/v1/search/queries", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["owner_handle"], "jqlsave");

    // Same name again is a conflict that says so, not a duplicate row.
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/search/queries",
        Some(&token),
        Some(json!({ "name": "my open WORK", "jql": "status = todo" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body:?}");

    // An unparseable query is refused at save time — storing it would just
    // hand the surprise to whoever loads it later.
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/search/queries",
        Some(&token),
        Some(json!({ "name": "broken", "jql": "banana = 3" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");

    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/api/v1/search/queries/{id}"),
        Some(&token),
        Some(json!({ "name": "Everything of mine", "is_shared": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["name"], "Everything of mine");
    assert_eq!(body["is_shared"], true);

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/search/queries/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/search/queries/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_shared_query_is_readable_by_all_and_editable_by_its_owner_only(pool: PgPool) {
    let app = app(pool);
    let (owner, _) = register(&app, "jqlowner").await;
    let (other, _) = register(&app, "jqlother").await;

    let (status, made) = send(
        &app,
        "POST",
        "/api/v1/search/queries",
        Some(&owner),
        Some(json!({ "name": "Team triage", "jql": "priority = p0", "is_shared": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made:?}");
    let id = made["id"].as_str().unwrap().to_string();

    let (status, body) = send(&app, "GET", "/api/v1/search/queries", Some(&other), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"][0]["name"], "Team triage");
    assert_eq!(body["items"][0]["is_mine"], false);
    assert_eq!(body["items"][0]["owner_handle"], "jqlowner");

    // Sharing lends the query, not the pen.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/search/queries/{id}"),
        Some(&other),
        Some(json!({ "jql": "priority = p3" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/search/queries/{id}"),
        Some(&other),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A private query stays private.
    let (_, made2) = send(
        &app,
        "POST",
        "/api/v1/search/queries",
        Some(&owner),
        Some(json!({ "name": "Mine only", "jql": "status = todo" })),
    )
    .await;
    assert_eq!(made2["is_shared"], false);
    let (_, body) = send(&app, "GET", "/api/v1/search/queries", Some(&other), None).await;
    let names: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["Team triage"],
        "private query leaked: {names:?}"
    );
}

// ── task restore, the undo behind the delete toast (fix/qa4-feedback-pass) ──

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_task_can_be_restored(pool: PgPool) {
    let app = app(pool);
    // Burn the bootstrap admin so the others are ordinary members.
    let _ = register(&app, "undoroot").await;
    let (owner, _) = register(&app, "undoowner").await;
    make_project(&app, &owner, "UNDO").await;
    let key = make_task_http(&app, &owner, "UNDO", "delete me by accident").await;

    // Deletes are soft, which is what makes undo possible at all.
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/tasks/{key}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{key}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a deleted task must not be readable"
    );

    // Undo.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{key}/restore"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, task) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{key}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{task:?}");
    assert_eq!(task["title"], "delete me by accident");
    assert_eq!(task["key"], key);

    // Restoring something that isn't deleted has nothing to undo. A 404 keeps
    // a double-tapped toast from being reported as success.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{key}/restore"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A key that never existed, and one from a project the caller can't see,
    // are both refused — restore resolves deleted rows, so it needs the same
    // gate as every other write.
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/tasks/UNDO-999/restore",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (outsider, _) = register(&app, "undooutsider").await;
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/tasks/{key}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{key}/restore"),
        Some(&outsider),
        None,
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "a non-member restored someone else's task (got {status})"
    );

    // No session at all, either. This is a 403 rather than a 401 because the
    // CSRF guard sits in front of auth on writes and answers first — restore
    // isn't on its exemption list, and shouldn't be.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{key}/restore"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_jql_field_list_needs_a_session_like_its_siblings(pool: PgPool) {
    let app = app(pool);

    // It shipped unauthenticated by omission — the constant is harmless, but an
    // authenticated API shouldn't have a public corner nobody meant to open.
    let (status, _) = send(&app, "GET", "/api/v1/search/jql/fields", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (token, _) = register(&app, "jqlfields").await;
    let (status, body) = send(&app, "GET", "/api/v1/search/jql/fields", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert!(
        body["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "assignee"),
        "{body:?}"
    );
}

// ── email preferences + unsubscribe (feat/notification-emails) ────────────

#[sqlx::test(migrations = "./migrations")]
async fn email_prefs_default_to_personal_kinds_and_merge_on_update(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "mailprefs").await;

    let (status, p) = send(
        &app,
        "GET",
        "/api/v1/users/me/email-prefs",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p:?}");
    assert_eq!(p["mode"], "immediate");
    assert_eq!(p["kinds"]["mention"], true);
    assert_eq!(p["kinds"]["assigned"], true);
    // Comments are the chatty kind — opt-in, not opt-out.
    assert_eq!(p["kinds"]["comment"], false);
    // The UI shouldn't hardcode the kind list.
    assert!(p["available_kinds"].as_array().unwrap().len() >= 3);
    // No SMTP in tests, so the UI can say "logged, not sent" honestly.
    assert_eq!(p["delivery_configured"], false);

    // Toggling one kind must not drop the others.
    let (status, p) = send(
        &app,
        "PATCH",
        "/api/v1/users/me/email-prefs",
        Some(&token),
        Some(json!({ "kinds": { "comment": true } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p:?}");
    assert_eq!(p["kinds"]["comment"], true);
    assert_eq!(p["kinds"]["mention"], true, "merge dropped a key: {p:?}");

    let (status, p) = send(
        &app,
        "PATCH",
        "/api/v1/users/me/email-prefs",
        Some(&token),
        Some(json!({ "mode": "digest", "digest_hour": 17 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p:?}");
    assert_eq!(p["mode"], "digest");
    assert_eq!(p["digest_hour"], 17);

    // Junk is refused with a message, not stored.
    for bad in [
        json!({ "mode": "sometimes" }),
        json!({ "digest_hour": 25 }),
        json!({ "kinds": { "telepathy": true } }),
        json!({ "kinds": { "mention": "yes" } }),
    ] {
        let (status, body) = send(
            &app,
            "PATCH",
            "/api/v1/users/me/email-prefs",
            Some(&token),
            Some(bad.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} accepted: {body:?}");
    }

    let (status, _) = send(&app, "GET", "/api/v1/users/me/email-prefs", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_notification_enqueues_exactly_one_email_job(pool: PgPool) {
    let app = app(pool.clone());
    let (alice, _) = register(&app, "mailalice").await;
    let (bob, bob_user) = register(&app, "mailbob").await;
    make_project(&app, &alice, "MAIL").await;

    // Bob needs to be a member before he can be assigned anything.
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/projects/MAIL/members",
        Some(&alice),
        Some(json!({ "user_id": bob_user["id"], "role": "contributor" })),
    )
    .await;
    assert!(status.is_success(), "adding bob failed: {status}");

    let key = make_task_http(&app, &alice, "MAIL", "please look at this").await;
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/tasks/{key}"),
        Some(&alice),
        Some(json!({ "assignee_id": bob_user["id"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The notification exists…
    let notes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE user_id = $1 AND kind = 'assigned'",
    )
    .bind(uuid::Uuid::parse_str(bob_user["id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(notes, 1);

    // …and so does exactly one queued email for it. The worker decides whether
    // to send; enqueueing is what the request path is responsible for.
    let jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'send_notification_email' AND finished_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(jobs, 1, "expected one queued notification email");

    let _ = bob;
}

#[sqlx::test(migrations = "./migrations")]
async fn unsubscribe_is_one_way_and_needs_no_session(pool: PgPool) {
    let app = app(pool.clone());
    let (token, user) = register(&app, "mailunsub").await;
    let uid = uuid::Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();

    // Rows are created lazily — by the first preference write, or by the worker
    // before it sends the first email. That way every account-creation path
    // (register, invite, admin-provisioned, OIDC) is covered without each one
    // having to remember. Touching prefs here is what mints the token.
    let (status, _) = send(
        &app,
        "PATCH",
        "/api/v1/users/me/email-prefs",
        Some(&token),
        Some(json!({ "kinds": { "comment": true } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let raw: Vec<u8> =
        sqlx::query_scalar("SELECT unsubscribe_token FROM email_prefs WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();

    // Followed from a mail client: no session, and it answers with a page.
    let (status, _) = send(
        &app,
        "GET",
        &format!("/api/v1/email/unsubscribe?token={hex}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mode: String = sqlx::query_scalar("SELECT mode FROM email_prefs WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(mode, "off");

    // A token that matches nothing is answered identically — no oracle.
    let other = "ab".repeat(32);
    let (status, _) = send(
        &app,
        "GET",
        &format!("/api/v1/email/unsubscribe?token={other}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Malformed links say so rather than 500ing.
    for bad in ["", "zz", "abc"] {
        let (status, _) = send(
            &app,
            "GET",
            &format!("/api/v1/email/unsubscribe?token={bad}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "token `{bad}`");
    }

    // Turning it back on is a thing only the account holder can do.
    let (status, p) = send(
        &app,
        "PATCH",
        "/api/v1/users/me/email-prefs",
        Some(&token),
        Some(json!({ "mode": "immediate" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p:?}");
    assert_eq!(p["mode"], "immediate");
}

// ── attachments follow the request origin (fix/attachments-same-origin) ─────

/// Like `app()`, but with `MINIO_PUBLIC_ENDPOINT` in its path form.
fn app_with_path_s3(pool: PgPool) -> Router {
    let mut cfg = test_config();
    cfg.minio.public_endpoint = "/s3".into();
    cfg.public_url = "http://fallback.test".into();
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

/// `send`, plus arbitrary request headers — what a reverse proxy adds.
async fn send_with_headers(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for (k, v) in extra {
        builder = builder.header(*k, *v);
    }
    let mut req = match body {
        Some(j) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(j.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
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

/// QA reports 5 and 6: attachments must work on whatever address the app was
/// opened at — and behind a TLS-terminating CDN, where the proxy chain claims
/// `http`. Path-form endpoints produce *relative* upload URLs (the page's own
/// scheme and host), signed for the host the request arrived with; downloads
/// go through a stable API link that re-signs at click time.
#[sqlx::test(migrations = "./migrations")]
async fn attachment_urls_are_signed_for_the_host_the_browser_used(pool: PgPool) {
    let app = app_with_path_s3(pool);
    let (token, _) = register(&app, "attorigin").await;
    make_project(&app, &token, "ATT").await;
    let key = make_task_http(&app, &token, "ATT", "needs a file").await;

    let body =
        json!({ "filename": "spec.pdf", "mime_type": "application/pdf", "size_bytes": 1234 });

    // Opened by hostname behind a CDN that terminates TLS: the proxy says
    // `http`, the page is `https`. An absolute http:// URL is exactly what the
    // browser blocked; a relative one can't disagree with the page.
    for headers in [
        vec![
            ("x-forwarded-proto", "http"),
            ("x-forwarded-host", "sprintly.example"),
        ],
        vec![
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "212.33.206.34:8083"),
        ],
        vec![],
        // A forged scheme/host can't smuggle anything into the URL either.
        vec![
            ("x-forwarded-proto", "javascript"),
            ("x-forwarded-host", "evil.example/../"),
        ],
    ] {
        let (status, a) = send_with_headers(
            &app,
            "POST",
            &format!("/api/v1/tasks/{key}/attachments"),
            Some(&token),
            Some(body.clone()),
            &headers,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{a:?}");
        let url = a["upload_url"].as_str().expect("upload_url");
        assert!(
            url.starts_with("/s3/sprintly/tasks/"),
            "path-form endpoints give relative URLs, got {url} for {headers:?}"
        );
        assert!(url.contains("X-Amz-Signature="), "{url}");
    }

    // Finish one upload; the list hands out the stable download link, not a
    // presigned URL that expires while the page sits open.
    let (status, a) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{key}/attachments"),
        Some(&token),
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = a["id"].as_str().unwrap().to_string();
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/attachments/{id}/complete"),
        Some(&token),
        Some(json!({ "size_bytes": 1234 })),
    )
    .await;
    assert!(status.is_success());
    let (_, list) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{key}/attachments"),
        Some(&token),
        None,
    )
    .await;
    let ready = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id.as_str())
        .unwrap();
    assert_eq!(
        ready["download_url"],
        format!("/api/v1/attachments/{id}/download")
    );
    // Unfinished uploads have no download link at all.
    assert!(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["status"] == "pending")
        .all(|i| i["download_url"].is_null()));

    // Following it: a fresh, short-lived, relative presigned URL, uncached.
    let (status, headers, _) =
        send_raw(&app, &format!("/api/v1/attachments/{id}/download"), &token).await;
    assert_eq!(status, StatusCode::FOUND);
    let loc = headers[header::LOCATION].to_str().unwrap();
    assert!(loc.starts_with("/s3/sprintly/tasks/"), "{loc}");
    assert!(loc.contains("X-Amz-Expires=120"), "{loc}");
    assert!(loc.contains("response-content-disposition="), "{loc}");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");

    // Access is checked at click time: an outsider gets nothing.
    let (stranger, _) = register(&app, "attstranger").await;
    let (status, _, _) = send_raw(
        &app,
        &format!("/api/v1/attachments/{id}/download"),
        &stranger,
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "{status}"
    );
    // A pending upload can't be downloaded.
    let pending = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["status"] == "pending")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _, _) = send_raw(
        &app,
        &format!("/api/v1/attachments/{pending}/download"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

// ── subtask counts ride along on every list (QA5 item 3) ─────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn every_task_list_carries_a_live_subtask_count(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "subcount").await;
    make_project(&app, &token, "SUB").await;
    let parent = make_task_http(&app, &token, "SUB", "the parent").await;
    let lonely = make_task_http(&app, &token, "SUB", "no children").await;

    // Two subtasks, then delete one — the count must be of *live* children.
    let (_, parent_row) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{parent}"),
        Some(&token),
        None,
    )
    .await;
    let parent_id = parent_row["id"].as_str().expect("parent id").to_string();
    let mut kids = vec![];
    for t in ["kid one", "kid two"] {
        let (status, k) = send(
            &app,
            "POST",
            "/api/v1/projects/SUB/tasks",
            Some(&token),
            Some(json!({ "title": t, "parent_task_id": parent_id })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{k:?}");
        kids.push(k["key"].as_str().unwrap().to_string());
    }
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/tasks/{}", kids[1]),
        Some(&token),
        None,
    )
    .await;
    assert!(status.is_success(), "delete kid: {status}");

    // Single task.
    let (_, t) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{parent}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(t["subtask_count"], 1, "{t:?}");
    let (_, l) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{lonely}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(l["subtask_count"], 0);

    // Board / project list.
    let (_, list) = send(
        &app,
        "GET",
        "/api/v1/projects/SUB/tasks",
        Some(&token),
        None,
    )
    .await;
    let items = list["items"]
        .as_array()
        .or_else(|| list.as_array())
        .expect("task list");
    let by_key = |k: &str| {
        items
            .iter()
            .find(|i| i["key"] == k)
            .cloned()
            .expect("task in list")
    };
    assert_eq!(by_key(&parent)["subtask_count"], 1);
    assert_eq!(by_key(&lonely)["subtask_count"], 0);

    // Backlog.
    let (_, backlog) = send(
        &app,
        "GET",
        "/api/v1/projects/SUB/backlog",
        Some(&token),
        None,
    )
    .await;
    let rows = backlog.as_array().expect("backlog array");
    let row = rows
        .iter()
        .find(|r| r["key"] == parent)
        .expect("parent in backlog");
    assert_eq!(row["subtask_count"], 1, "{row:?}");
}

// ── task report export: docx + pdf over HTTP (QA5 coverage gap) ──────────────

/// `send`, but hands back the raw body and headers — the report routes return
/// binary, not JSON.
async fn send_raw(
    app: &Router,
    path: &str,
    token: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut req = Request::builder()
        .method("GET")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            8080,
        ))));
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = to_bytes(resp.into_body(), 8 << 20).await.unwrap().to_vec();
    (status, headers, bytes)
}

#[sqlx::test(migrations = "./migrations")]
async fn task_report_exports_as_real_docx_and_pdf(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "reporter").await;
    make_project(&app, &token, "RPT").await;
    let parent = make_task_http(
        &app,
        &token,
        "RPT",
        "Ship the (thing) — with <tags> & \"quotes\"",
    )
    .await;
    let (_, parent_row) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{parent}"),
        Some(&token),
        None,
    )
    .await;
    let parent_id = parent_row["id"].as_str().unwrap().to_string();
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/projects/RPT/tasks",
        Some(&token),
        Some(json!({ "title": "a subtask in the report", "parent_task_id": parent_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Word: a real zip (local-file header magic), served as a .docx download.
    let (status, h, body) = send_raw(&app, "/api/v1/projects/RPT/export?format=docx", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    );
    let cd = h
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(cd.contains("RPT-report.docx"), "{cd}");
    assert_eq!(&body[..4], b"PK\x03\x04", "docx must be a zip");
    // The document text lives in word/document.xml inside the zip; stored
    // (uncompressed) entries let us see it straight in the bytes, and the
    // writer XML-escapes — angle brackets from the title must not appear raw.
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("word/document.xml"), "missing document part");
    assert!(
        text.contains("a subtask in the report"),
        "subtasks are part of the report"
    );
    assert!(
        !text.contains("<tags>"),
        "title markup must be escaped inside the XML"
    );

    // PDF: magic header, served as a .pdf download, mentions the project.
    let (status, h, body) = send_raw(&app, "/api/v1/projects/RPT/export?format=pdf", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        Some("application/pdf")
    );
    let cd = h
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(cd.contains("RPT-report.pdf"), "{cd}");
    assert!(body.starts_with(b"%PDF-"), "pdf magic");
    assert!(
        body.windows(5).any(|w| w == b"%%EOF"),
        "pdf must be terminated"
    );

    // Members only: a stranger gets nothing.
    let (other, _) = register(&app, "nosy").await;
    let (status, _, _) = send_raw(&app, "/api/v1/projects/RPT/export?format=pdf", &other).await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "expected 403/404 for a non-member, got {status}"
    );
}

// ── completed sprints keep their history (QA5 item 8) ────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_completed_sprint_remembers_its_tasks_even_after_carry_over(pool: PgPool) {
    let app = app(pool.clone());
    let (token, user) = register(&app, "snapper").await;
    make_project(&app, &token, "SNAP").await;
    let done_key = make_task_http(&app, &token, "SNAP", "finished in time").await;
    let open_key = make_task_http(&app, &token, "SNAP", "spilled over").await;

    let sprint = make_sprint(&app, &token, "SNAP", "Sprint 1").await;
    for k in [&done_key, &open_key] {
        let (status, _) = send(
            &app,
            "POST",
            &format!("/api/v1/sprints/{sprint}/tasks/{k}"),
            Some(&token),
            None,
        )
        .await;
        assert!(status.is_success(), "assign {k}: {status}");
    }
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint}/start"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Pretend the sprint has been running for a while, so a log from two
    // hours ago falls *inside* it. Only time during the sprint counts —
    // that's the metric QA asked for, not the task's lifetime total.
    sqlx::query("UPDATE sprints SET started_at = now() - interval '3 hours' WHERE id = $1::uuid")
        .bind(&sprint)
        .execute(&pool)
        .await
        .unwrap();

    // Mark one done, log 90 minutes on the other while the sprint runs.
    let cols = columns(&app, &token, "SNAP").await;
    let done_col = cols
        .iter()
        .find(|(_, c)| c == "done")
        .map(|(id, _)| id.clone())
        .expect("done column");
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{done_key}/move"),
        Some(&token),
        Some(json!({ "column_id": done_col })),
    )
    .await;
    assert!(status.is_success(), "move to done: {status}");
    let open_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM tasks WHERE key = $1")
        .bind(&open_key)
        .fetch_one(&pool)
        .await
        .unwrap();
    let uid = uuid::Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, note, billable)
         VALUES ($1, $2, $3, now() - interval '2 hours', now() - interval '30 minutes', 'work', false)",
    )
    .bind(uuid::Uuid::now_v7()).bind(open_id).bind(uid)
    .execute(&pool).await.unwrap();

    // Complete, carrying the leftover to the backlog — the move that used to
    // erase it from this sprint's history.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{sprint}/complete"),
        Some(&token),
        Some(json!({ "carry_over": { "to": "backlog" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["carried_over"], 1);

    // The live task really did leave…
    let live_sprint: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT sprint_id FROM tasks WHERE key = $1")
            .bind(&open_key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        live_sprint.is_none(),
        "carry-over should have moved the open task"
    );

    // …but the sprint still lists both, as they stood, with the sprint's time.
    let (status, tasks) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{sprint}/tasks"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tasks:?}");
    assert_eq!(tasks["snapshot"], true, "{tasks:?}");
    let items = tasks["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{tasks:?}");
    let row = |k: &str| items.iter().find(|i| i["key"] == k).cloned().expect(k);
    assert_eq!(row(&done_key)["status"], "done");
    assert_eq!(
        row(&open_key)["status"],
        "todo",
        "state at completion, not now"
    );
    assert_eq!(row(&open_key)["logged_minutes"], 90);
    assert_eq!(row(&done_key)["logged_minutes"], 0);

    // Even deleting the task afterwards doesn't rewrite the sprint.
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/tasks/{open_key}"),
        Some(&token),
        None,
    )
    .await;
    assert!(status.is_success());
    let (_, tasks2) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{sprint}/tasks"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        tasks2["items"].as_array().unwrap().len(),
        2,
        "history is history"
    );

    // An open sprint is still the live view.
    let s2 = make_sprint(&app, &token, "SNAP", "Sprint 2").await;
    let (_, live) = send(
        &app,
        "GET",
        &format!("/api/v1/sprints/{s2}/tasks"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(live["snapshot"], false);
}

/// QA report 6: label names are editable. Tasks hold labels by name, so a
/// rename has to follow onto every tagged task — otherwise they keep the old
/// name, lose its colour, and stop matching `label:<new>`.
#[sqlx::test(migrations = "./migrations")]
async fn renaming_a_label_renames_it_on_every_task(pool: PgPool) {
    let app = app(pool.clone());
    let (token, _) = register(&app, "relabeler").await;
    make_project(&app, &token, "RLB").await;

    let (status, label) = send(
        &app,
        "POST",
        "/api/v1/projects/RLB/labels",
        Some(&token),
        Some(json!({ "name": "front", "color": "#7c5cff" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{label:?}");
    let id = label["id"].as_str().unwrap().to_string();

    let tagged = make_task_http(&app, &token, "RLB", "tagged").await;
    let both = make_task_http(&app, &token, "RLB", "already has the new name").await;
    let other = make_task_http(&app, &token, "RLB", "someone else's label").await;
    for (key, labels) in [
        (&tagged, json!(["urgent", "FRONT"])),
        (&both, json!(["front", "frontend"])),
        (&other, json!(["back"])),
    ] {
        let (s, b) = send(
            &app,
            "PATCH",
            &format!("/api/v1/tasks/{key}"),
            Some(&token),
            Some(json!({ "labels": labels })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{b:?}");
    }

    let (status, renamed) = send(
        &app,
        "PATCH",
        &format!("/api/v1/labels/{id}"),
        Some(&token),
        Some(json!({ "name": "frontend" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed:?}");
    assert_eq!(renamed["name"], "frontend");
    assert_eq!(renamed["color"], "#7c5cff", "a rename keeps the colour");

    let labels_of = |key: String| {
        let app = app.clone();
        let token = token.clone();
        async move {
            let (_, t) = send(
                &app,
                "GET",
                &format!("/api/v1/tasks/{key}"),
                Some(&token),
                None,
            )
            .await;
            t["labels"].clone()
        }
    };
    // Case-insensitive match, position kept.
    assert_eq!(
        labels_of(tagged.clone()).await,
        json!(["urgent", "frontend"])
    );
    // The rename would have produced a duplicate; one copy survives.
    assert_eq!(labels_of(both.clone()).await, json!(["frontend"]));
    // Unrelated labels are untouched.
    assert_eq!(labels_of(other.clone()).await, json!(["back"]));

    // A colour-only edit renames nothing.
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/labels/{id}"),
        Some(&token),
        Some(json!({ "color": "#22d3ee" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(labels_of(tagged).await, json!(["urgent", "frontend"]));

    // Renaming onto a name the palette already has is a conflict, not a merge.
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/projects/RLB/labels",
        Some(&token),
        Some(json!({ "name": "design", "color": "#10b981" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = send(
        &app,
        "PATCH",
        &format!("/api/v1/labels/{id}"),
        Some(&token),
        Some(json!({ "name": "Design" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// QA report 6: "my open work in the running sprint", the way Jira people
/// write it — `sprint is active` used to be a parse error at character 59.
#[sqlx::test(migrations = "./migrations")]
async fn jql_finds_my_open_work_in_the_active_sprint(pool: PgPool) {
    let app = app(pool);
    let (token, me) = register(&app, "sprinter").await;
    let my_id = me["id"].as_str().unwrap().to_string();
    make_project(&app, &token, "SIA").await;
    let cols = columns(&app, &token, "SIA").await;
    let done_col = cols.iter().find(|(_, c)| c == "done").unwrap().0.clone();

    let running = make_sprint(&app, &token, "SIA", "Running").await;
    let later = make_sprint(&app, &token, "SIA", "Later").await;

    let mine_open = make_task_http(&app, &token, "SIA", "mine, open, running sprint").await;
    let mine_done = make_task_http(&app, &token, "SIA", "mine, done, running sprint").await;
    let mine_later = make_task_http(&app, &token, "SIA", "mine, open, future sprint").await;
    let mine_backlog = make_task_http(&app, &token, "SIA", "mine, open, no sprint").await;
    let nobodys = make_task_http(&app, &token, "SIA", "unassigned, running sprint").await;

    for key in [&mine_open, &mine_done, &mine_later, &mine_backlog] {
        let (s, b) = send(
            &app,
            "PATCH",
            &format!("/api/v1/tasks/{key}"),
            Some(&token),
            Some(json!({ "assignee_id": my_id })),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{b:?}");
    }
    for (sprint, key) in [
        (&running, &mine_open),
        (&running, &mine_done),
        (&running, &nobodys),
        (&later, &mine_later),
    ] {
        let (s, _) = send(
            &app,
            "POST",
            &format!("/api/v1/sprints/{sprint}/tasks/{key}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::NO_CONTENT);
    }
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/v1/tasks/{mine_done}/move"),
        Some(&token),
        Some(json!({ "column_id": done_col })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = send(
        &app,
        "POST",
        &format!("/api/v1/sprints/{running}/start"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");

    let (status, body) = jql(
        &app,
        &token,
        "assignee = currentUser() AND status != done AND sprint is active ORDER BY priority ASC",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(keys(&body), vec![mine_open.clone()]);

    // Jira's spelling of the same thing.
    let (_, body) = jql(
        &app,
        &token,
        "assignee = currentUser() AND sprint in openSprints()",
    )
    .await;
    let mut got = keys(&body);
    got.sort();
    let mut want = vec![mine_open.clone(), mine_done.clone(), mine_later.clone()];
    want.sort();
    assert_eq!(got, want, "openSprints() is active + not yet started");

    // "Not in the running sprint" includes the backlog.
    let (_, body) = jql(
        &app,
        &token,
        "assignee = currentUser() AND sprint is not active",
    )
    .await;
    let mut got = keys(&body);
    got.sort();
    let mut want = vec![mine_later, mine_backlog];
    want.sort();
    assert_eq!(got, want);
}

/// QA report 6: subtasks are reorderable by drag-and-drop, and the order is
/// stored. New subtasks land after the arranged ones; a stale list is refused.
#[sqlx::test(migrations = "./migrations")]
async fn subtasks_keep_the_order_they_were_dragged_into(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "arranger").await;
    make_project(&app, &token, "SUB").await;
    let parent = make_task_http(&app, &token, "SUB", "the parent").await;
    let mut kids = Vec::new();
    for t in ["one", "two", "three"] {
        let child = make_task_http(&app, &token, "SUB", t).await;
        assert_eq!(
            set_parent(&app, &token, &child, Some(&parent)).await,
            StatusCode::NO_CONTENT
        );
        kids.push(child);
    }
    let order = |app: Router, token: String, parent: String| async move {
        let (s, body) = send(
            &app,
            "GET",
            &format!("/api/v1/tasks/{parent}/subtasks"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["key"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        order(app.clone(), token.clone(), parent.clone()).await,
        kids
    );

    let wanted = vec![kids[2].clone(), kids[0].clone(), kids[1].clone()];
    let (s, b) = send(
        &app,
        "PUT",
        &format!("/api/v1/tasks/{parent}/subtasks/order"),
        Some(&token),
        Some(json!({ "keys": wanted })),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{b:?}");
    assert_eq!(
        order(app.clone(), token.clone(), parent.clone()).await,
        wanted
    );

    // A new subtask goes after the arranged ones.
    let four = make_task_http(&app, &token, "SUB", "four").await;
    assert_eq!(
        set_parent(&app, &token, &four, Some(&parent)).await,
        StatusCode::NO_CONTENT
    );
    let mut with_four = wanted.clone();
    with_four.push(four.clone());
    assert_eq!(
        order(app.clone(), token.clone(), parent.clone()).await,
        with_four
    );

    // A list that leaves one out (someone added a subtask meanwhile) is a
    // conflict, and changes nothing.
    let (s, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/tasks/{parent}/subtasks/order"),
        Some(&token),
        Some(json!({ "keys": [&kids[0], &kids[1], &kids[2]] })),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // So is one naming a stranger, or the same key twice.
    let (s, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/tasks/{parent}/subtasks/order"),
        Some(&token),
        Some(json!({ "keys": [&kids[0], &kids[0], &kids[1], &kids[2], &four] })),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(
        order(app.clone(), token.clone(), parent.clone()).await,
        with_four
    );

    // Moving a subtask to another parent forgets its old position.
    let other = make_task_http(&app, &token, "SUB", "other parent").await;
    let only = make_task_http(&app, &token, "SUB", "only child").await;
    assert_eq!(
        set_parent(&app, &token, &only, Some(&other)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_parent(&app, &token, &kids[2], Some(&other)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        order(app.clone(), token.clone(), other.clone()).await,
        vec![only, kids[2].clone()],
        "a newcomer lands at the end, not at its old slot"
    );

    // Someone outside the project can't reorder it.
    let (stranger, _) = register(&app, "outsider").await;
    let (s, _) = send(
        &app,
        "PUT",
        &format!("/api/v1/tasks/{parent}/subtasks/order"),
        Some(&stranger),
        Some(json!({ "keys": with_four })),
    )
    .await;
    assert!(
        s == StatusCode::FORBIDDEN || s == StatusCode::NOT_FOUND,
        "{s}"
    );
}

/// QA report 6: the team manages a task's watchers — not only "watch me".
/// Leads and contributors add/remove teammates; read-only members only
/// themselves; nobody outside the project can be added.
#[sqlx::test(migrations = "./migrations")]
async fn the_team_manages_who_watches_a_task(pool: PgPool) {
    let app = app(pool);
    let (lead, _) = register(&app, "wlead").await;
    let (mate, mate_user) = register(&app, "wmate").await;
    let (reader, reader_user) = register(&app, "wreader").await;
    let (_, outsider_user) = register(&app, "woutsider").await;
    make_project(&app, &lead, "WAT").await;
    for (u, role) in [(&mate_user, "contributor"), (&reader_user, "watcher")] {
        let (s, b) = send(
            &app,
            "POST",
            "/api/v1/projects/WAT/members",
            Some(&lead),
            Some(json!({ "user_id": u["id"], "role": role })),
        )
        .await;
        assert!(s.is_success(), "{b:?}");
    }
    let task = make_task_http(&app, &lead, "WAT", "watch this").await;
    let add = |token: String, user_id: Value| {
        let app = app.clone();
        let task = task.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/v1/tasks/{task}/watchers"),
                Some(&token),
                Some(json!({ "user_id": user_id })),
            )
            .await
            .0
        }
    };
    let watchers = || {
        let app = app.clone();
        let task = task.clone();
        let lead = lead.clone();
        async move {
            let (_, b) = send(
                &app,
                "GET",
                &format!("/api/v1/tasks/{task}/watchers"),
                Some(&lead),
                None,
            )
            .await;
            let mut h: Vec<String> = b["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| w["handle"].as_str().unwrap().to_string())
                .collect();
            h.sort();
            h
        }
    };

    // The lead adds a contributor; the contributor adds the read-only member.
    assert_eq!(
        add(lead.clone(), mate_user["id"].clone()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        add(mate.clone(), reader_user["id"].clone()).await,
        StatusCode::NO_CONTENT
    );
    // The lead is on the list already: reporters auto-watch what they file.
    assert_eq!(watchers().await, vec!["wlead", "wmate", "wreader"]);

    // Someone who isn't on the project can't be made to watch it.
    assert_eq!(
        add(lead.clone(), outsider_user["id"].clone()).await,
        StatusCode::BAD_REQUEST
    );
    // Nor can a made-up user id (used to be an FK violation → 500).
    assert_eq!(
        add(lead.clone(), json!(uuid::Uuid::now_v7().to_string())).await,
        StatusCode::BAD_REQUEST
    );

    // A read-only member watches themselves, but doesn't manage others.
    let (_, lead_me) = send(&app, "GET", "/api/v1/users/me", Some(&lead), None).await;
    assert_eq!(
        add(reader.clone(), lead_me["id"].clone()).await,
        StatusCode::FORBIDDEN
    );
    let (s, _) = send(
        &app,
        "DELETE",
        &format!(
            "/api/v1/tasks/{task}/watchers/{}",
            mate_user["id"].as_str().unwrap()
        ),
        Some(&reader),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &app,
        "DELETE",
        &format!(
            "/api/v1/tasks/{task}/watchers/{}",
            reader_user["id"].as_str().unwrap()
        ),
        Some(&reader),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "anyone can stop watching");

    // A contributor takes a teammate off.
    let (s, _) = send(
        &app,
        "DELETE",
        &format!(
            "/api/v1/tasks/{task}/watchers/{}",
            mate_user["id"].as_str().unwrap()
        ),
        Some(&mate),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(watchers().await, vec!["wlead"]);
}

/// QA report 6: a safe way to delete a project. Lead only, the key must be
/// typed out, everything in it disappears at once — and an admin can bring
/// it back exactly as it was (without resurrecting tasks deleted earlier).
#[sqlx::test(migrations = "./migrations")]
async fn deleting_a_project_is_confirmed_hidden_and_restorable(pool: PgPool) {
    let app = app(pool);
    // First account is the bootstrap admin.
    let (admin, _) = register(&app, "padmin").await;
    let (lead, _) = register(&app, "plead").await;
    let (mate, mate_user) = register(&app, "pmate").await;
    make_project(&app, &lead, "DEL").await;
    let (s, _) = send(
        &app,
        "POST",
        "/api/v1/projects/DEL/members",
        Some(&lead),
        Some(json!({ "user_id": mate_user["id"], "role": "contributor" })),
    )
    .await;
    assert!(s.is_success());
    let keep = make_task_http(&app, &lead, "DEL", "comes back").await;
    let gone = make_task_http(&app, &lead, "DEL", "was already deleted").await;
    let (s, _) = send(
        &app,
        "DELETE",
        &format!("/api/v1/tasks/{gone}"),
        Some(&lead),
        None,
    )
    .await;
    assert!(s.is_success());

    let del = |token: String, confirm: &'static str| {
        let app = app.clone();
        async move {
            send(
                &app,
                "DELETE",
                "/api/v1/projects/DEL",
                Some(&token),
                Some(json!({ "confirm": confirm })),
            )
            .await
        }
    };

    // A contributor can't, whatever they type.
    assert_eq!(del(mate.clone(), "DEL").await.0, StatusCode::FORBIDDEN);
    // The lead has to type the key exactly.
    let (s, b) = del(lead.clone(), "del").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(
        b["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("DEL"),
        "{b:?}"
    );
    assert_eq!(del(lead.clone(), "").await.0, StatusCode::BAD_REQUEST);
    // And then it's gone — the project, its tasks, from every angle.
    assert_eq!(del(lead.clone(), "DEL").await.0, StatusCode::NO_CONTENT);
    let (s, _) = send(&app, "GET", "/api/v1/projects/DEL", Some(&lead), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, list) = send(&app, "GET", "/api/v1/projects", Some(&lead), None).await;
    assert!(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["key"] != "DEL"));
    let (s, _) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{keep}"),
        Some(&lead),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Deleting twice is a 404, not a second audit row.
    assert_eq!(del(lead.clone(), "DEL").await.0, StatusCode::NOT_FOUND);

    // Only admins see the list, and it says what would come back.
    let (s, _) = send(
        &app,
        "GET",
        "/api/v1/admin/deleted-projects",
        Some(&lead),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, deleted) = send(
        &app,
        "GET",
        "/api/v1/admin/deleted-projects",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{deleted:?}");
    let row = deleted["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "DEL")
        .cloned()
        .expect("listed");
    assert_eq!(
        row["task_count"], 1,
        "the earlier-deleted task isn't counted"
    );
    assert_eq!(row["key_taken"], false);
    let id = row["id"].as_str().unwrap().to_string();

    // The key is free for a new project meanwhile; restoring then conflicts.
    make_project(&app, &lead, "DEL").await;
    let (s, _) = send(
        &app,
        "GET",
        "/api/v1/admin/deleted-projects",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/v1/admin/deleted-projects/{id}/restore"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // Free the key again (delete the newcomer), then restore the original.
    assert_eq!(del(lead.clone(), "DEL").await.0, StatusCode::NO_CONTENT);
    let (s, b) = send(
        &app,
        "POST",
        &format!("/api/v1/admin/deleted-projects/{id}/restore"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    assert_eq!(b["tasks"], 1);
    let (s, t) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{keep}"),
        Some(&lead),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t:?}");
    assert_eq!(t["title"], "comes back");
    let (s, _) = send(
        &app,
        "GET",
        &format!("/api/v1/tasks/{gone}"),
        Some(&lead),
        None,
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "deleted before the project, stays deleted"
    );

    // Both ends are in the audit log.
    let (_, audit) = send(&app, "GET", "/api/v1/admin/audit", Some(&admin), None).await;
    let actions: Vec<&str> = audit["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["action"].as_str())
        .collect();
    assert!(actions.contains(&"project.deleted"), "{actions:?}");
    assert!(actions.contains(&"project.restored"), "{actions:?}");
}

/// QA report 6: an epic opens to show its tasks. The list is exactly what the
/// bar's "done/total" counts, unfinished work first.
#[sqlx::test(migrations = "./migrations")]
async fn an_epic_lists_the_tasks_its_progress_counts(pool: PgPool) {
    let app = app(pool);
    let (token, _) = register(&app, "epicist").await;
    make_project(&app, &token, "EPC").await;
    let cols = columns(&app, &token, "EPC").await;
    let done_col = cols.iter().find(|(_, c)| c == "done").unwrap().0.clone();
    let (s, epic) = send(
        &app,
        "POST",
        "/api/v1/projects/EPC/epics",
        Some(&token),
        Some(json!({ "name": "Phase 3" })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{epic:?}");
    let epic_id = epic["id"].as_str().unwrap().to_string();

    let mut keys = Vec::new();
    for (i, t) in ["a", "b", "c", "d"].iter().enumerate() {
        let k = make_task_http(&app, &token, "EPC", t).await;
        let (s, _) = send(
            &app,
            "PUT",
            &format!("/api/v1/tasks/{k}/epic"),
            Some(&token),
            Some(json!({ "epic_id": epic_id })),
        )
        .await;
        assert!(s.is_success());
        if i < 3 {
            let (s, _) = send(
                &app,
                "POST",
                &format!("/api/v1/tasks/{k}/move"),
                Some(&token),
                Some(json!({ "column_id": done_col })),
            )
            .await;
            assert_eq!(s, StatusCode::OK);
        }
        keys.push(k);
    }
    // A task outside the epic, and a deleted one inside it, don't show.
    make_task_http(&app, &token, "EPC", "elsewhere").await;
    let ghost = make_task_http(&app, &token, "EPC", "ghost").await;
    send(
        &app,
        "PUT",
        &format!("/api/v1/tasks/{ghost}/epic"),
        Some(&token),
        Some(json!({ "epic_id": epic_id })),
    )
    .await;
    send(
        &app,
        "DELETE",
        &format!("/api/v1/tasks/{ghost}"),
        Some(&token),
        None,
    )
    .await;

    let (s, list) = send(
        &app,
        "GET",
        &format!("/api/v1/epics/{epic_id}/tasks"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{list:?}");
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["key"], keys[3], "the open one leads");
    assert_eq!(items[0]["status"], "todo");
    assert_eq!(items.iter().filter(|i| i["status"] == "done").count(), 3);

    // And the bar's numbers agree.
    let (_, epics) = send(
        &app,
        "GET",
        "/api/v1/projects/EPC/epics",
        Some(&token),
        None,
    )
    .await;
    let e = epics
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == epic_id.as_str())
        .unwrap();
    assert_eq!(
        (e["done_count"].as_i64(), e["task_count"].as_i64()),
        (Some(3), Some(4))
    );

    // Outsiders can't peek.
    let (other, _) = register(&app, "epicoutsider").await;
    let (s, _) = send(
        &app,
        "GET",
        &format!("/api/v1/epics/{epic_id}/tasks"),
        Some(&other),
        None,
    )
    .await;
    assert!(
        s == StatusCode::FORBIDDEN || s == StatusCode::NOT_FOUND,
        "{s}"
    );
}
