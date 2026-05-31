//! M7 integration tests.

use sprintly_api::{
    config::AuthConfig,
    domain::{
        password,
        vault::{self as vc, ProjectKey},
    },
};
use sqlx::PgPool;
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

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, 'member')"#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", &id.to_string()[..8]))
    .bind(format!("h{}", &id.to_string()[..8]))
    .bind("Test")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> Uuid {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid)
        .bind(key)
        .bind(key)
        .bind(owner)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, 'lead', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    pid
}

#[sqlx::test(migrations = "./migrations")]
async fn ciphertext_round_trips_through_db(pool: PgPool) {
    let owner = make_user(&pool).await;
    let pid = make_project(&pool, "VLT", owner).await;
    let item_id = Uuid::now_v7();
    let master = [7u8; 32];
    let pkey = ProjectKey::derive(&master, pid, 1);
    let (ct, nonce) = vc::encrypt(&pkey, b"supersecret123", item_id.as_bytes()).unwrap();
    sqlx::query(
        r#"INSERT INTO vault_items
              (id, project_id, name, kind, encrypted_payload, nonce, key_version, created_by)
           VALUES ($1, $2, 'a', 'password', $3, $4, 1, $5)"#,
    )
    .bind(item_id)
    .bind(pid)
    .bind(&ct)
    .bind(nonce.as_slice())
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();

    // Read back and decrypt.
    let row: (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT encrypted_payload, nonce FROM vault_items WHERE id = $1")
            .bind(item_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let pt = vc::decrypt(&pkey, &row.0, &row.1, item_id.as_bytes()).unwrap();
    assert_eq!(pt, b"supersecret123");
}

#[sqlx::test(migrations = "./migrations")]
async fn nonce_length_check_enforced(pool: PgPool) {
    let owner = make_user(&pool).await;
    let pid = make_project(&pool, "NLEN", owner).await;
    let bad_nonce = vec![0u8; 12]; // 12 != 24
    let res = sqlx::query(
        r#"INSERT INTO vault_items
              (id, project_id, name, kind, encrypted_payload, nonce, key_version, created_by)
           VALUES ($1, $2, 'b', 'password', $3, $4, 1, $5)"#,
    )
    .bind(Uuid::now_v7())
    .bind(pid)
    .bind(vec![1u8; 32])
    .bind(bad_nonce.as_slice())
    .bind(owner)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "schema CHECK must reject non-24-byte nonces");
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_log_is_append_only(pool: PgPool) {
    let owner = make_user(&pool).await;
    let pid = make_project(&pool, "AUD", owner).await;
    let item_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO vault_items
              (id, project_id, name, kind, encrypted_payload, nonce, key_version, created_by)
           VALUES ($1, $2, 'c', 'password', $3, $4, 1, $5)"#,
    )
    .bind(item_id).bind(pid).bind(vec![1u8; 32]).bind(vec![0u8; 24]).bind(owner)
    .execute(&pool).await.unwrap();

    let audit_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO vault_audit_log
              (id, vault_item_id, user_id, action) VALUES ($1, $2, $3, 'revealed')"#,
    )
    .bind(audit_id).bind(item_id).bind(owner)
    .execute(&pool).await.unwrap();

    let upd = sqlx::query("UPDATE vault_audit_log SET action = 'edited' WHERE id = $1")
        .bind(audit_id)
        .execute(&pool)
        .await;
    assert!(upd.is_err(), "UPDATE must be rejected by trigger");

    let del = sqlx::query("DELETE FROM vault_audit_log WHERE id = $1")
        .bind(audit_id)
        .execute(&pool)
        .await;
    assert!(del.is_err(), "DELETE must be rejected by trigger");
}

#[sqlx::test(migrations = "./migrations")]
async fn item_name_unique_per_project(pool: PgPool) {
    let owner = make_user(&pool).await;
    let pid = make_project(&pool, "UNQ", owner).await;
    let mk = |name: &str| {
        let p = pid;
        let o = owner;
        let n = name.to_string();
        async move {
            sqlx::query(
                r#"INSERT INTO vault_items
                      (id, project_id, name, kind, encrypted_payload, nonce, key_version, created_by)
                   VALUES ($1, $2, $3, 'password', $4, $5, 1, $6)"#,
            )
            .bind(Uuid::now_v7()).bind(p).bind(n).bind(vec![1u8; 32]).bind(vec![0u8; 24]).bind(o)
            .execute(&pool).await
        }
    };
    mk("db").await.unwrap();
    let dup = mk("db").await;
    assert!(dup.is_err(), "duplicate name within project must collide");

    // Soft-delete + re-create with same name should succeed.
    sqlx::query("UPDATE vault_items SET deleted_at = now() WHERE project_id = $1 AND name = 'db'")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    mk("db").await.unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn access_pk_dedupes(pool: PgPool) {
    let owner = make_user(&pool).await;
    let other = make_user(&pool).await;
    let pid = make_project(&pool, "ACCS", owner).await;
    let item_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO vault_items
              (id, project_id, name, kind, encrypted_payload, nonce, key_version, created_by)
           VALUES ($1, $2, 'd', 'password', $3, $4, 1, $5)"#,
    )
    .bind(item_id).bind(pid).bind(vec![1u8; 32]).bind(vec![0u8; 24]).bind(owner)
    .execute(&pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO vault_access (vault_item_id, user_id, can_view, can_edit)
           VALUES ($1, $2, true, false)"#,
    )
    .bind(item_id).bind(other).execute(&pool).await.unwrap();
    let dup = sqlx::query(
        r#"INSERT INTO vault_access (vault_item_id, user_id, can_view, can_edit)
           VALUES ($1, $2, true, true)"#,
    )
    .bind(item_id).bind(other).execute(&pool).await;
    assert!(dup.is_err(), "(item, user) PK must dedupe — use UPSERT");
}
