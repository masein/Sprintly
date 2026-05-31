//! M10 integration tests — admin guards, audit immutability, backups,
//! and the XFF helper from `middleware::client_ip`.

use sprintly_api::{
    config::AuthConfig,
    domain::password,
    middleware::client_ip,
};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::str::FromStr;
use uuid::Uuid;

fn cfg() -> AuthConfig {
    AuthConfig {
        jwt_secret: b"a-test-secret-that-is-long-enough-to-be-fine".to_vec(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 2_592_000,
        argon2_m_cost_kib: 4096,
        argon2_t_cost: 1,
        argon2_p_cost: 1,
    }
}

async fn make_user(pool: &PgPool, role: &str) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", &id.to_string()[..8]))
    .bind(format!("h{}", &id.to_string()[..8]))
    .bind("Test")
    .bind(&hash)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_audit_log_is_append_only(pool: PgPool) {
    let admin = make_user(&pool, "admin").await;
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO admin_audit_log (id, actor_id, action, payload)
           VALUES ($1, $2, 'user.suspend', '{}'::jsonb)"#,
    )
    .bind(id)
    .bind(admin)
    .execute(&pool)
    .await
    .unwrap();

    let upd = sqlx::query("UPDATE admin_audit_log SET action = 'tamper' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(upd.is_err(), "audit log UPDATE must be blocked by trigger");

    let del = sqlx::query("DELETE FROM admin_audit_log WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(del.is_err(), "audit log DELETE must be blocked by trigger");
}

#[sqlx::test(migrations = "./migrations")]
async fn backup_status_transitions(pool: PgPool) {
    let admin = make_user(&pool, "admin").await;
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO backups (id, requested_by, status) VALUES ($1, $2, 'pending')"#,
    )
    .bind(id)
    .bind(admin)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("UPDATE backups SET status = 'running', started_at = now() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        r#"UPDATE backups SET status = 'done', finished_at = now(), size_bytes = 12345,
                              storage_key = 'backups/2026-05-25/x.dump'
           WHERE id = $1"#,
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, size, key): (String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT status, size_bytes, storage_key FROM backups WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "done");
    assert_eq!(size, Some(12345));
    assert_eq!(key.as_deref(), Some("backups/2026-05-25/x.dump"));

    // CHECK constraint rejects unknown status values.
    let bad = sqlx::query("UPDATE backups SET status = 'corrupted' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(bad.is_err(), "status CHECK must reject unknown values");
}

#[sqlx::test(migrations = "./migrations")]
async fn backup_status_check_rejects_arbitrary(pool: PgPool) {
    let admin = make_user(&pool, "admin").await;
    let res = sqlx::query(
        r#"INSERT INTO backups (id, requested_by, status) VALUES ($1, $2, 'unknown')"#,
    )
    .bind(Uuid::now_v7())
    .bind(admin)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "CHECK rejects out-of-enum status");
}

#[sqlx::test(migrations = "./migrations")]
async fn webhooks_per_project_isolation(pool: PgPool) {
    let owner = make_user(&pool, "member").await;
    let p1 = Uuid::now_v7();
    let p2 = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'W1', 'W1', $2)"#)
        .bind(p1).bind(owner).execute(&pool).await.unwrap();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'W2', 'W2', $2)"#)
        .bind(p2).bind(owner).execute(&pool).await.unwrap();

    for pid in [p1, p2] {
        sqlx::query(
            r#"INSERT INTO webhooks (id, project_id, url, secret_hash, events)
               VALUES ($1, $2, 'https://example.test/wh', $3, '{task.created}')"#,
        )
        .bind(Uuid::now_v7())
        .bind(pid)
        .bind(vec![0u8; 32])
        .execute(&pool)
        .await
        .unwrap();
    }

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM webhooks WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(p1)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "each project sees only its own webhook");
}

// ─── XFF parser unit checks ─────────────────────────────────────────────────

fn xff(v: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("x-forwarded-for", HeaderValue::from_str(v).unwrap());
    h
}
fn ci(addr: &str) -> ConnectInfo<SocketAddr> {
    ConnectInfo(SocketAddr::from_str(addr).unwrap())
}

#[test]
fn xff_skips_private_addrs() {
    let ip = client_ip(&xff("10.0.0.5, 8.8.8.8"), ci("127.0.0.1:80"));
    assert_eq!(ip.to_string(), "8.8.8.8");
}

#[test]
fn xff_handles_garbage() {
    let ip = client_ip(&xff("garbage, 1.1.1.1"), ci("127.0.0.1:80"));
    assert_eq!(ip.to_string(), "1.1.1.1");
}

#[test]
fn xff_falls_back_to_socket_without_header() {
    let ip = client_ip(&HeaderMap::new(), ci("203.0.113.42:80"));
    assert_eq!(ip.to_string(), "203.0.113.42");
}
