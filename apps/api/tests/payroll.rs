//! M8 integration tests — payroll aggregation + burn-rate math.

use chrono::{Duration, NaiveDate, Utc};
use sprintly_api::{
    config::AuthConfig,
    domain::{password, payroll::{self, BurnStatus}, tasks as task_domain},
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

async fn make_project_with_task(pool: &PgPool, key: &str, owner: Uuid) -> Uuid {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid).bind(key).bind(key).bind(owner).execute(pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, 'lead', $2)"#,
    ).bind(pid).bind(owner).execute(pool).await.unwrap();
    let board = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    ).bind(board).bind(pid).execute(pool).await.unwrap();
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'C', 'todo', 1024.0)"#,
    ).bind(col).bind(board).execute(pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let (k, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 1024.0)"#,
    )
    .bind(Uuid::now_v7()).bind(pid).bind(board).bind(col).bind(&k)
    .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    pid
}

#[sqlx::test(migrations = "./migrations")]
async fn project_budget_check_rejects_negative(pool: PgPool) {
    let owner = make_user(&pool, 5000).await;
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid).bind("BUD").bind("BUD").bind(owner).execute(&pool).await.unwrap();
    let res = sqlx::query("UPDATE projects SET budget_cents = -100 WHERE id = $1")
        .bind(pid)
        .execute(&pool)
        .await;
    assert!(res.is_err(), "CHECK must reject negative budget");
}

#[sqlx::test(migrations = "./migrations")]
async fn payroll_period_unique_per_user_per_month(pool: PgPool) {
    let owner = make_user(&pool, 5000).await;
    sqlx::query(
        r#"INSERT INTO payroll_periods (user_id, period_year, period_month, status)
           VALUES ($1, 2026, 5, 'open')"#,
    )
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
    let dup = sqlx::query(
        r#"INSERT INTO payroll_periods (user_id, period_year, period_month, status)
           VALUES ($1, 2026, 5, 'open')"#,
    )
    .bind(owner)
    .execute(&pool)
    .await;
    assert!(dup.is_err(), "(user, year, month) PK must dedupe");
}

#[sqlx::test(migrations = "./migrations")]
async fn time_in_month_sums_billable_only_when_filtered(pool: PgPool) {
    let owner = make_user(&pool, 6000).await; // $60/hr
    let pid = make_project_with_task(&pool, "TIM", owner).await;
    let task_id: Uuid =
        sqlx::query_scalar("SELECT id FROM tasks WHERE project_id = $1")
            .bind(pid).fetch_one(&pool).await.unwrap();

    // Pick a fixed Wednesday in May 2026.
    use chrono::TimeZone;
    let in_month = Utc.with_ymd_and_hms(2026, 5, 13, 9, 0, 0).unwrap();
    // 60 min billable.
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, billable)
           VALUES ($1, $2, $3, $4, $5, true)"#,
    )
    .bind(Uuid::now_v7()).bind(task_id).bind(owner)
    .bind(in_month).bind(in_month + Duration::minutes(60))
    .execute(&pool).await.unwrap();
    // 30 min non-billable.
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, billable)
           VALUES ($1, $2, $3, $4, $5, false)"#,
    )
    .bind(Uuid::now_v7()).bind(task_id).bind(owner)
    .bind(in_month + Duration::hours(2)).bind(in_month + Duration::hours(2) + Duration::minutes(30))
    .execute(&pool).await.unwrap();
    // 45 min in a different month (April).
    let out = Utc.with_ymd_and_hms(2026, 4, 13, 9, 0, 0).unwrap();
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, billable)
           VALUES ($1, $2, $3, $4, $5, true)"#,
    )
    .bind(Uuid::now_v7()).bind(task_id).bind(owner)
    .bind(out).bind(out + Duration::minutes(45))
    .execute(&pool).await.unwrap();

    let (first, last) = payroll::month_bounds(2026, 5).unwrap();
    use chrono::{NaiveDateTime, NaiveTime};
    let start_ts = Utc.from_utc_datetime(&NaiveDateTime::new(first, NaiveTime::MIN));
    let end_ts = Utc.from_utc_datetime(&NaiveDateTime::new(
        last + Duration::days(1),
        NaiveTime::MIN,
    ));

    // Replicate the aggregation query at the row-level.
    let totals: (i64, i64) = sqlx::query_as(
        r#"
        SELECT COALESCE(SUM(duration_minutes), 0)::bigint,
               COALESCE(SUM(duration_minutes) FILTER (WHERE billable = true), 0)::bigint
        FROM   time_logs
        WHERE  user_id = $1
          AND  deleted_at IS NULL AND ended_at IS NOT NULL
          AND  started_at >= $2 AND started_at < $3
        "#,
    )
    .bind(owner)
    .bind(start_ts)
    .bind(end_ts)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(totals.0, 90, "may total = 60 + 30");
    assert_eq!(totals.1, 60, "may billable = 60");
    assert_eq!(payroll::pay_cents(60, Some(6000)), 6000);
}

#[test]
fn burn_status_thresholds() {
    // No budget → None.
    assert_eq!(payroll::burn_status(100, None, 0.5), BurnStatus::None);
    // Over budget → Over.
    assert_eq!(payroll::burn_status(101, Some(100), 0.1), BurnStatus::Over);
    // Spent < elapsed × 1.10 → Ok.
    assert_eq!(payroll::burn_status(20, Some(100), 0.20), BurnStatus::Ok);
    // Spent > elapsed × 1.10 → Warn.
    assert_eq!(payroll::burn_status(50, Some(100), 0.20), BurnStatus::Warn);
}

#[test]
fn month_shift_round_trips() {
    let (y, m) = payroll::month_shift(2026, 1, -1);
    let (y2, m2) = payroll::month_shift(y, m, 1);
    assert_eq!((y2, m2), (2026, 1));
}

#[test]
fn pdf_builder_emits_valid_minimal_pdf() {
    use sprintly_api::infra::pdf::PdfBuilder;
    let mut b = PdfBuilder::new();
    b.text(50.0, 700.0, 12.0, "Sprintly payroll smoke");
    let bytes = b.finish();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
}
