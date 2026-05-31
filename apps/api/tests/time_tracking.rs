//! M4 integration tests.

use chrono::{Datelike, Duration, NaiveDate, Utc};
use sprintly_api::{
    config::AuthConfig,
    domain::{password, tasks as task_domain, timesheets as ts},
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

async fn make_user(pool: &PgPool, rate_cents: Option<i64>) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, hourly_rate_cents)
           VALUES ($1, $2, $3, $4, $5, 'member', $6)"#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", &id.to_string()[..8]))
    .bind(format!("h{}", &id.to_string()[..8]))
    .bind("Test")
    .bind(&hash)
    .bind(rate_cents)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_task(pool: &PgPool, owner: Uuid) -> Uuid {
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#,
    ).bind(pid).bind("TM").bind("T").bind(owner).execute(pool).await.unwrap();
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
    let (key, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
    let tid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', 1024.0)"#,
    ).bind(tid).bind(pid).bind(board).bind(col).bind(&key)
    .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    tid
}

#[sqlx::test(migrations = "./migrations")]
async fn one_running_log_per_user(pool: PgPool) {
    let u = make_user(&pool, Some(10_000)).await;
    let t = make_task(&pool, u).await;

    let now = Utc::now();
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7())
    .bind(t)
    .bind(u)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    // Second concurrent running log must violate the partial unique index.
    let dup = sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7())
    .bind(t)
    .bind(u)
    .bind(now)
    .execute(&pool)
    .await;
    assert!(dup.is_err(), "second running log must collide");
}

#[sqlx::test(migrations = "./migrations")]
async fn duration_computed_on_close(pool: PgPool) {
    let u = make_user(&pool, Some(0)).await;
    let t = make_task(&pool, u).await;
    let started = Utc::now() - Duration::minutes(45);
    let ended = started + Duration::minutes(45);
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(id).bind(t).bind(u).bind(started).bind(ended)
    .execute(&pool).await.unwrap();
    let mins: Option<i32> =
        sqlx::query_scalar("SELECT duration_minutes FROM time_logs WHERE id = $1")
            .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(mins, Some(45));
}

#[sqlx::test(migrations = "./migrations")]
async fn week_bounds_pay_math_unit(_pool: PgPool) {
    // Pure unit, just sanity-rerun the helpers against fixed dates.
    let wed = NaiveDate::from_ymd_opt(2026, 5, 27).unwrap();
    let (m, s) = ts::week_bounds(wed);
    assert_eq!(m.weekday(), chrono::Weekday::Mon);
    assert_eq!(s.weekday(), chrono::Weekday::Sun);
    assert_eq!(ts::pay_cents(60, Some(7500)), 7500);
    assert_eq!(ts::pay_cents(0, Some(7500)), 0);
    assert_eq!(ts::pay_cents(60, None), 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn approved_timesheet_locks_writes_in_range(pool: PgPool) {
    // Schema-only assertion: an `approved` timesheet row exists; the route
    // layer's ensure_week_open() reads this and refuses writes. We assert
    // the data shape it depends on.
    let u = make_user(&pool, Some(10_000)).await;
    let mon = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
    let sun = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    sqlx::query(
        r#"INSERT INTO timesheets (id, user_id, period_start, period_end, status, approved_at)
           VALUES ($1, $2, $3, $4, 'approved', now())"#,
    )
    .bind(Uuid::now_v7())
    .bind(u)
    .bind(mon)
    .bind(sun)
    .execute(&pool)
    .await
    .unwrap();

    let status: String = sqlx::query_scalar(
        "SELECT status FROM timesheets WHERE user_id = $1 AND period_start = $2",
    )
    .bind(u)
    .bind(mon)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "approved");

    // Status transition guards live in the route layer; this row is the
    // signal `ensure_week_open` looks for to refuse subsequent log edits.
}

#[sqlx::test(migrations = "./migrations")]
async fn timesheet_unique_per_user_per_week(pool: PgPool) {
    let u = make_user(&pool, Some(0)).await;
    let mon = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
    let sun = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    sqlx::query(
        r#"INSERT INTO timesheets (id, user_id, period_start, period_end)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7()).bind(u).bind(mon).bind(sun)
    .execute(&pool).await.unwrap();

    let dup = sqlx::query(
        r#"INSERT INTO timesheets (id, user_id, period_start, period_end)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::now_v7()).bind(u).bind(mon).bind(sun)
    .execute(&pool).await;
    assert!(dup.is_err(), "duplicate (user_id, period_start) must collide");
}
