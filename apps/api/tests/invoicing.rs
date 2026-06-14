//! Integration tests for per-client invoicing (F14): a generated invoice's
//! line items and total match the underlying billable time × each contributor's
//! rate, and the draft→sent→paid lifecycle holds.

use chrono::{Duration, NaiveDate, TimeZone, Utc};
use sprintly_api::{
    config::AuthConfig,
    domain::{invoicing, password, tasks as task_domain},
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

async fn make_user(pool: &PgPool, rate: i64) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, hourly_rate_cents)
           VALUES ($1, $2, $3, $4, $5, 'member', $6)"#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", id.simple()))
    .bind(format!("h{}", id.simple()))
    .bind("Test")
    .bind(&hash)
    .bind(rate)
    .execute(pool)
    .await
    .unwrap();
    id
}

/// A project + default board/column + one task, optionally linked to a client.
async fn make_project_with_task(
    pool: &PgPool,
    key: &str,
    owner: Uuid,
    client_id: Option<Uuid>,
) -> (Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by, client_id) VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(pid)
    .bind(key)
    .bind(key)
    .bind(owner)
    .bind(client_id)
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
           VALUES ($1, $2, 'C', 'todo', 1024.0)"#,
    )
    .bind(col)
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let (k, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    let task_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 1024.0)"#,
    )
    .bind(task_id)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(&k)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (pid, task_id)
}

async fn log(pool: &PgPool, task: Uuid, user: Uuid, minutes: i64, billable: bool) {
    let start = Utc.with_ymd_and_hms(2026, 5, 13, 9, 0, 0).unwrap()
        + Duration::minutes((user.as_u128() % 100) as i64 * 60);
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, billable)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(Uuid::now_v7())
    .bind(task)
    .bind(user)
    .bind(start)
    .bind(start + Duration::minutes(minutes))
    .bind(billable)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn invoice_totals_match_time_times_rate(pool: PgPool) {
    let admin = make_user(&pool, 0).await;
    let alice = make_user(&pool, 6000).await; // $60/hr
    let bob = make_user(&pool, 9000).await; // $90/hr

    let client = invoicing::create_client(
        &pool,
        admin,
        invoicing::NewClient {
            name: "Acme".into(),
            email: Some("ap@acme.test".into()),
            address: None,
            currency: Some("usd".into()),
            notes: None,
        },
    )
    .await
    .unwrap();

    let (_pid, task) = make_project_with_task(&pool, "ACME", admin, Some(client.id)).await;
    log(&pool, task, alice, 60, true).await; // 60m × $60 = 6000¢
    log(&pool, task, bob, 30, true).await; //   30m × $90 = 4500¢
    log(&pool, task, alice, 30, false).await; // non-billable → excluded

    let period_start = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let id = invoicing::generate(&pool, client.id, period_start, period_end, admin)
        .await
        .unwrap();

    let inv = invoicing::fetch(&pool, id).await.unwrap();
    assert_eq!(inv.invoice.currency, "USD");
    assert_eq!(inv.lines.len(), 2, "one line per (project, contributor)");
    // Total equals the sum of the line amounts, which equal minutes × rate ÷ 60.
    let line_sum: i64 = inv.lines.iter().map(|l| l.amount_cents).sum();
    assert_eq!(line_sum, 6000 + 4500);
    assert_eq!(inv.invoice.total_cents, 10_500);
    assert_eq!(inv.invoice.subtotal_cents, 10_500);
    assert!(inv.invoice.number.starts_with("INV-2026-"));

    // Lifecycle: draft → sent → paid. A paid invoice can't be deleted.
    assert_eq!(inv.invoice.status, "draft");
    invoicing::mark_sent(&pool, id).await.unwrap();
    invoicing::mark_paid(&pool, id).await.unwrap();
    let paid = invoicing::fetch(&pool, id).await.unwrap();
    assert_eq!(paid.invoice.status, "paid");
    assert!(paid.invoice.paid_at.is_some());
    assert!(invoicing::delete_draft(&pool, id).await.is_err());
    // Marking paid again is a no-op conflict.
    assert!(invoicing::mark_paid(&pool, id).await.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn generate_rejects_when_no_billable_time(pool: PgPool) {
    let admin = make_user(&pool, 0).await;
    let user = make_user(&pool, 5000).await;
    let client = invoicing::create_client(
        &pool,
        admin,
        invoicing::NewClient {
            name: "Empty Co".into(),
            email: None,
            address: None,
            currency: None,
            notes: None,
        },
    )
    .await
    .unwrap();
    let (_pid, task) = make_project_with_task(&pool, "EMP", admin, Some(client.id)).await;
    log(&pool, task, user, 45, false).await; // only non-billable

    let start = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    assert!(invoicing::generate(&pool, client.id, start, end, admin)
        .await
        .is_err());
}
