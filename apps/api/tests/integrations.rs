//! F1 — Git integration: linking commits/PRs to tasks, provider connections,
//! multi-provider inbound (GitHub + GitLab fixtures), and outbound status job
//! enqueueing. (Pure signature/parse/status-request shapes are unit-tested in
//! the domain modules.)

use sprintly_api::domain::{
    git_providers::{self, Provider},
    integrations,
};
use sqlx::PgPool;
use uuid::Uuid;

const MASTER_KEY: &[u8; 32] = b"0123456789abcdef0123456789abcdef";

/// Create a task with a known key and return its id.
async fn make_task(pool: &PgPool, key: &str) -> Uuid {
    let owner = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'T', 'x', 'member', 'active')"#,
    )
    .bind(owner)
    .bind(format!("{}@x.test", owner.simple()))
    .bind(format!("u{}", &owner.simple().to_string()[..10]))
    .execute(pool)
    .await
    .unwrap();
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'GH', 'GitHub', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    let board = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    )
    .bind(board)
    .bind(pid)
    .execute(pool)
    .await
    .unwrap();
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'Todo', 'todo', 1024.0)"#,
    )
    .bind(col)
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    // A done column so merge auto-transition has somewhere to land.
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'Done', 'done', 4096.0)"#,
    )
    .bind(Uuid::now_v7())
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    let task = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 'todo', 1024.0)"#,
    )
    .bind(task)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(key)
    .execute(pool)
    .await
    .unwrap();
    task
}

async fn count(pool: &PgPool, sql: &str, task: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(task)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn link_creates_then_dedupes(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;

    let first = integrations::link(
        &pool,
        "github",
        None,
        "GH-1",
        "commit",
        "abc1234",
        Some("http://x/c"),
        Some("msg"),
        None,
        Some("abc1234def5678abc1234def5678abc1234def56"),
    )
    .await
    .unwrap();
    assert!(first, "first link is created");

    let second = integrations::link(
        &pool,
        "github",
        None,
        "GH-1",
        "commit",
        "abc1234",
        Some("http://x/c"),
        Some("msg"),
        None,
        Some("abc1234def5678abc1234def5678abc1234def56"),
    )
    .await
    .unwrap();
    assert!(!second, "same ref de-duplicates");

    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM git_links WHERE task_id = $1",
            task
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM task_activity WHERE task_id = $1 AND kind = 'commit_linked'",
            task
        )
        .await,
        1,
        "activity logged exactly once"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_key_is_noop(pool: PgPool) {
    make_task(&pool, "GH-1").await;
    let r = integrations::link(
        &pool, "github", None, "NOPE-9", "commit", "abc", None, None, None, None,
    )
    .await
    .unwrap();
    assert!(!r);
}

#[sqlx::test(migrations = "./migrations")]
async fn pr_merge_updates_state_and_logs(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    integrations::link(
        &pool,
        "github",
        None,
        "GH-1",
        "pull_request",
        "#5",
        Some("http://x/pr/5"),
        Some("My PR"),
        Some("open"),
        Some("feedfacefeedfacefeedfacefeedfacefeedface"),
    )
    .await
    .unwrap();
    integrations::link(
        &pool,
        "github",
        None,
        "GH-1",
        "pull_request",
        "#5",
        Some("http://x/pr/5"),
        Some("My PR"),
        Some("merged"),
        Some("feedfacefeedfacefeedfacefeedfacefeedface"),
    )
    .await
    .unwrap();

    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM task_activity WHERE task_id = $1 AND kind = 'pr_merged'",
            task
        )
        .await,
        1
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM git_links WHERE task_id = $1 AND kind = 'pull_request'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, "merged");

    // The merge auto-transitioned the task into the done column.
    let (status, category): (String, String) = sqlx::query_as(
        "SELECT t.status, bc.category FROM tasks t
         JOIN board_columns bc ON bc.id = t.column_id WHERE t.id = $1",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "done");
    assert_eq!(category, "done");

    // The head SHA was captured for outbound status.
    let sha: Option<String> = sqlx::query_scalar(
        "SELECT sha FROM git_links WHERE task_id = $1 AND kind = 'pull_request'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        sha.as_deref(),
        Some("feedfacefeedfacefeedfacefeedfacefeedface")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn integration_secrets_round_trip_and_stay_hidden(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();

    let gi = integrations::create_integration(
        &pool,
        MASTER_KEY,
        project_id,
        "github",
        "acme/app",
        None,
        Some("hook-secret"),
        Some("api-token-123"),
        true,
        None,
    )
    .await
    .unwrap();
    assert!(gi.has_webhook_secret);
    assert!(gi.has_api_token);

    // The DTO never leaks plaintext; decryption round-trips.
    let json = serde_json::to_string(&gi).unwrap();
    assert!(!json.contains("api-token-123"));
    assert!(!json.contains("hook-secret"));
    let token = integrations::decrypt_api_token(&pool, MASTER_KEY, gi.id)
        .await
        .unwrap();
    assert_eq!(token.as_deref(), Some("api-token-123"));

    // Same repo twice → conflict.
    assert!(integrations::create_integration(
        &pool, MASTER_KEY, project_id, "github", "acme/app", None, None, None, false, None,
    )
    .await
    .is_err());

    integrations::delete_integration(&pool, gi.id, project_id)
        .await
        .unwrap();
    assert!(integrations::list_integrations(&pool, project_id)
        .await
        .unwrap()
        .is_empty());
    // Token gone with the row.
    assert!(integrations::decrypt_api_token(&pool, MASTER_KEY, gi.id)
        .await
        .unwrap()
        .is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn status_jobs_enqueue_only_when_eligible(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();
    async fn jobs(pool: &PgPool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE kind = 'push_commit_status'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    // No integration yet: linking a commit (with SHA) enqueues nothing.
    integrations::link(
        &pool,
        "github",
        None,
        "GH-1",
        "commit",
        "abc1234",
        None,
        Some("m"),
        None,
        Some("abc1234def5678abc1234def5678abc1234def56"),
    )
    .await
    .unwrap();
    assert_eq!(jobs(&pool).await, 0, "no connection → no job");

    // Status-enabled connection with a token: the next link enqueues.
    integrations::create_integration(
        &pool,
        MASTER_KEY,
        project_id,
        "github",
        "acme/app",
        None,
        None,
        Some("tok"),
        true,
        None,
    )
    .await
    .unwrap();
    integrations::queue_status_updates(&pool, task)
        .await
        .unwrap();
    assert_eq!(jobs(&pool).await, 1, "eligible task → job enqueued");

    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE kind = 'push_commit_status' LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        payload.get("task_id").and_then(|v| v.as_str()),
        Some(task.to_string().as_str())
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn project_scope_confines_linking(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let other_project = Uuid::now_v7();

    // A scope that doesn't own GH-1 can't link it.
    let r = integrations::link(
        &pool,
        "github",
        Some(other_project),
        "GH-1",
        "commit",
        "abc1234",
        None,
        Some("m"),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!r, "out-of-scope key resolves to nothing");

    // The owning project can.
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();
    let r = integrations::link(
        &pool,
        "github",
        Some(project_id),
        "GH-1",
        "commit",
        "abc1234",
        None,
        Some("m"),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(r);
}

#[sqlx::test(migrations = "./migrations")]
async fn webhook_secret_round_trips_and_update_clears_token(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();

    let secret = integrations::mint_webhook_secret();
    assert_eq!(secret.len(), 64, "32 bytes hex");

    let gi = integrations::create_integration(
        &pool,
        MASTER_KEY,
        project_id,
        "gitlab",
        "group/app",
        Some("https://gitlab.acme.dev"),
        Some(&secret),
        None,
        false,
        None,
    )
    .await
    .unwrap();

    let (pid, stored) = integrations::decrypt_webhook_secret(&pool, MASTER_KEY, gi.id)
        .await
        .unwrap();
    assert_eq!(pid, project_id);
    assert_eq!(stored.as_deref(), Some(secret.as_str()));

    // Set a token, then clear it; flip status_enabled on the way.
    let up = integrations::update_integration(
        &pool,
        MASTER_KEY,
        gi.id,
        project_id,
        Some(Some("glpat-123")),
        Some(true),
    )
    .await
    .unwrap();
    assert!(up.has_api_token);
    assert!(up.status_enabled);
    assert_eq!(
        integrations::decrypt_api_token(&pool, MASTER_KEY, gi.id)
            .await
            .unwrap()
            .as_deref(),
        Some("glpat-123")
    );

    let up =
        integrations::update_integration(&pool, MASTER_KEY, gi.id, project_id, Some(None), None)
            .await
            .unwrap();
    assert!(!up.has_api_token, "explicit null clears the token");
    assert!(up.status_enabled, "untouched fields stay put");
}

#[sqlx::test(migrations = "./migrations")]
async fn gitlab_merge_request_fixture_links_and_transitions(pool: PgPool) {
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();

    // A realistic GitLab "Merge Request Hook" payload referencing GH-1.
    let body = br#"{
        "object_kind": "merge_request",
        "object_attributes": {
            "iid": 9,
            "title": "GH-1 wire it up",
            "description": "also touches GH-1",
            "url": "https://gitlab.example/acme/app/-/merge_requests/9",
            "state": "merged",
            "last_commit": { "id": "abc123def4567890" }
        }
    }"#;

    // Signature is a shared token; enforce it both ways.
    assert!(git_providers::verify_signature(
        Provider::Gitlab,
        "gltoken",
        "gltoken",
        body
    ));
    assert!(!git_providers::verify_signature(
        Provider::Gitlab,
        "gltoken",
        "nope",
        body
    ));

    let events = git_providers::parse_event(Provider::Gitlab, "Merge Request Hook", body).unwrap();
    let n = integrations::apply_events(&pool, "gitlab", Some(project_id), &events)
        .await
        .unwrap();
    assert_eq!(n, 1, "one PR link created from the fixture");

    let (provider, state, sha): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT provider, state, sha FROM git_links WHERE task_id = $1 AND kind = 'pull_request'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(provider, "gitlab");
    assert_eq!(state.as_deref(), Some("merged"));
    assert_eq!(sha.as_deref(), Some("abc123def4567890"));

    // Merge auto-transitioned the task to done.
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "done");
}

#[sqlx::test(migrations = "./migrations")]
async fn github_push_fixture_links_branch_and_commit(pool: PgPool) {
    // AC1: pushing a branch named like a task key links it.
    let task = make_task(&pool, "GH-1").await;
    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();

    let body = br#"{
        "ref": "refs/heads/GH-1-add-thing",
        "commits": [
            { "id": "deadbeefcafebabe", "message": "GH-1 start work", "url": "http://c/1" }
        ]
    }"#;
    let events = git_providers::parse_event(Provider::Github, "push", body).unwrap();
    let n = integrations::apply_events(&pool, "github", Some(project_id), &events)
        .await
        .unwrap();
    assert_eq!(n, 2, "branch + commit both link");

    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM git_links WHERE task_id = $1 ORDER BY kind")
            .bind(task)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(kinds, vec!["branch".to_string(), "commit".to_string()]);

    let branch_ref: String = sqlx::query_scalar(
        "SELECT external_ref FROM git_links WHERE task_id = $1 AND kind = 'branch'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(branch_ref, "GH-1-add-thing");
}

#[sqlx::test(migrations = "./migrations")]
async fn same_pr_number_distinct_per_provider(pool: PgPool) {
    // The (task, provider, kind, ref) unique key lets GitHub #5 and GitLab !5
    // coexist on one task without colliding.
    let task = make_task(&pool, "GH-1").await;
    let pid: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task)
        .fetch_one(&pool)
        .await
        .unwrap();

    let gh = git_providers::parse_event(
        Provider::Github,
        "pull_request",
        br#"{"action":"opened","pull_request":{"number":5,"title":"GH-1 a","body":"","html_url":"http://gh/5","merged":false,"head":{"sha":"aa"}}}"#,
    )
    .unwrap();
    let gl = git_providers::parse_event(
        Provider::Gitlab,
        "Merge Request Hook",
        br#"{"object_attributes":{"iid":5,"title":"GH-1 b","description":"","url":"http://gl/5","state":"opened","last_commit":{"id":"bb"}}}"#,
    )
    .unwrap();
    integrations::apply_events(&pool, "github", Some(pid), &gh)
        .await
        .unwrap();
    integrations::apply_events(&pool, "gitlab", Some(pid), &gl)
        .await
        .unwrap();

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM git_links WHERE task_id = $1 AND kind = 'pull_request'",
    )
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 2, "both providers' PR #5 coexist");
}
