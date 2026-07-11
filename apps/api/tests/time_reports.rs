//! Integration tests for the flexible time report: real SQL fetch (range +
//! sprint + self filters) folded through the aggregator, and CSV parity.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use sprintly_api::domain::time_report;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool, handle: &str, rate_cents: Option<i64>) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status, hourly_rate_cents, currency)
           VALUES ($1, $2, $3, $4, 'x', 'member', 'active', $5, 'USD')"#,
    )
    .bind(id)
    .bind(format!("{handle}@x.test"))
    .bind(handle)
    .bind(handle)
    .bind(rate_cents)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $2, $3)"#)
        .bind(pid)
        .bind(key)
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
    sqlx::query(r#"INSERT INTO board_columns (id, board_id, name, category, sort_order) VALUES ($1, $2, 'Todo', 'todo', 1.0)"#)
        .bind(col)
        .bind(board)
        .execute(pool)
        .await
        .unwrap();
    (pid, board, col)
}

async fn make_sprint(
    pool: &PgPool,
    pid: Uuid,
    name: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at) VALUES ($1, $2, $3, $4, $5)"#)
        .bind(id)
        .bind(pid)
        .bind(name)
        .bind(ts(start, 9))
        .bind(ts(end, 9))
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn make_task(
    pool: &PgPool,
    pid: Uuid,
    board: Uuid,
    col: Uuid,
    key: &str,
    sprint: Option<Uuid>,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, order_in_column, sprint_id)
           VALUES ($1, $2, $3, $4, $5, $6, 'todo', 1024.0, $7)"#,
    )
    .bind(id)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(key)
    .bind(format!("{key} title"))
    .bind(sprint)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn log(pool: &PgPool, task: Uuid, user: Uuid, day: NaiveDate, minutes: i64, billable: bool) {
    let started = ts(day, 9);
    let ended = started + chrono::Duration::minutes(minutes);
    sqlx::query(
        r#"INSERT INTO time_logs (id, task_id, user_id, started_at, ended_at, billable)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(Uuid::now_v7())
    .bind(task)
    .bind(user)
    .bind(started)
    .bind(ended)
    .bind(billable)
    .execute(pool)
    .await
    .unwrap();
}

fn ts(d: NaiveDate, hour: u32) -> DateTime<Utc> {
    Utc.from_utc_datetime(&NaiveDateTime::new(
        d,
        NaiveTime::from_hms_opt(hour, 0, 0).unwrap(),
    ))
}
fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}
fn window(from: NaiveDate, to: NaiveDate) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    (
        Some(ts(from, 0)),
        Some(Utc.from_utc_datetime(&NaiveDateTime::new(
            to + chrono::Duration::days(1),
            NaiveTime::MIN,
        ))),
    )
}

/// Seed a fixed fixture and return (project_id, sprint_id, alice, bob).
async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let owner = make_user(pool, "owner", None).await;
    let alice = make_user(pool, "alice", Some(6000)).await; // $60/hr
    let bob = make_user(pool, "bob", Some(3000)).await; //   $30/hr
    let (pid, board, col) = make_project(pool, "TR", owner).await;
    let sprint = make_sprint(pool, pid, "Sprint 1", day(2026, 1, 5), day(2026, 1, 19)).await;
    let t1 = make_task(pool, pid, board, col, "TR-1", Some(sprint)).await; // in sprint
    let t2 = make_task(pool, pid, board, col, "TR-2", None).await; // no sprint

    // January logs.
    log(pool, t1, alice, day(2026, 1, 6), 60, true).await; // sprint, billable
    log(pool, t2, alice, day(2026, 1, 7), 30, false).await; // no sprint, non-billable
    log(pool, t1, bob, day(2026, 1, 8), 120, true).await; // sprint, billable
                                                          // A February log on the same sprint task — outside the January range.
    log(pool, t1, alice, day(2026, 2, 1), 45, true).await;

    (pid, sprint, alice, bob)
}

#[sqlx::test(migrations = "./migrations")]
async fn custom_range_totals_and_breakdowns(pool: PgPool) {
    let (pid, sprint, alice, _bob) = seed(&pool).await;
    let (start, end) = window(day(2026, 1, 1), day(2026, 1, 31));

    let logs = time_report::fetch_logs(&pool, pid, None, start, end, None)
        .await
        .unwrap();
    let r = time_report::aggregate(&logs);

    // Three January logs; the February 45 is excluded by the range.
    assert_eq!(r.total_minutes, 210);
    assert_eq!(r.billable_minutes, 180);
    // alice: 60 billable @ $60/hr = 6000c; bob: 120 @ $30/hr = 6000c.
    assert_eq!(r.total_pay_cents, 12000);
    assert_eq!(r.currency, "USD");

    // by_user sorted by total desc: bob (120) then alice (90).
    assert_eq!(r.by_user[0].handle, "bob");
    assert_eq!(r.by_user[0].total_minutes, 120);
    assert_eq!(r.by_user[1].handle, "alice");
    assert_eq!(r.by_user[1].total_minutes, 90);
    assert_eq!(r.by_user[1].billable_minutes, 60);

    // by_sprint: Sprint 1 (180) then the no-sprint bucket (30).
    assert_eq!(r.by_sprint[0].sprint_id, Some(sprint));
    assert_eq!(r.by_sprint[0].total_minutes, 180);
    assert_eq!(r.by_sprint[1].sprint_id, None);
    assert_eq!(r.by_sprint[1].total_minutes, 30);

    // by_task: TR-1 (180) then TR-2 (30).
    assert_eq!(r.by_task[0].task_key, "TR-1");
    assert_eq!(r.by_task[0].total_minutes, 180);

    // Self scope: alice only sees her own 90 minutes.
    let mine = time_report::aggregate(
        &time_report::fetch_logs(&pool, pid, None, start, end, Some(alice))
            .await
            .unwrap(),
    );
    assert_eq!(mine.total_minutes, 90);
    assert_eq!(mine.by_user.len(), 1);
    assert_eq!(mine.by_user[0].handle, "alice");
}

#[sqlx::test(migrations = "./migrations")]
async fn sprint_scope_ignores_date_and_combines_with_range(pool: PgPool) {
    let (pid, sprint, _alice, _bob) = seed(&pool).await;

    // Sprint only (no date window): every log on the sprint's task, incl. Feb.
    let all_sprint = time_report::aggregate(
        &time_report::fetch_logs(&pool, pid, Some(sprint), None, None, None)
            .await
            .unwrap(),
    );
    assert_eq!(all_sprint.total_minutes, 225); // 60 + 120 + 45
    assert_eq!(all_sprint.billable_minutes, 225);
    // Only the two sprint members; the no-sprint bucket never appears here.
    assert_eq!(all_sprint.by_sprint.len(), 1);
    assert_eq!(all_sprint.by_sprint[0].sprint_id, Some(sprint));

    // Sprint + January range → the Feb 45 drops out.
    let (start, end) = window(day(2026, 1, 1), day(2026, 1, 31));
    let jan_sprint = time_report::aggregate(
        &time_report::fetch_logs(&pool, pid, Some(sprint), start, end, None)
            .await
            .unwrap(),
    );
    assert_eq!(jan_sprint.total_minutes, 180); // 60 + 120
}

#[sqlx::test(migrations = "./migrations")]
async fn csv_matches_the_report(pool: PgPool) {
    let (pid, _sprint, _alice, _bob) = seed(&pool).await;
    let (start, end) = window(day(2026, 1, 1), day(2026, 1, 31));
    let logs = time_report::fetch_logs(&pool, pid, None, start, end, None)
        .await
        .unwrap();
    let data = time_report::aggregate(&logs);
    let csv = time_report::to_csv(&logs, data.total_minutes);

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "date,user,task_key,task_title,sprint,billable,minutes"
    );
    // Chronological rows for the three January logs.
    assert_eq!(
        lines[1],
        "2026-01-06,alice,TR-1,TR-1 title,Sprint 1,true,60"
    );
    assert_eq!(lines[2], "2026-01-07,alice,TR-2,TR-2 title,,false,30");
    assert_eq!(lines[3], "2026-01-08,bob,TR-1,TR-1 title,Sprint 1,true,120");
    // The TOTAL line equals the report's total_minutes.
    assert!(csv
        .trim_end()
        .ends_with(&format!("TOTAL,,,,,,{}", data.total_minutes)));
    assert_eq!(data.total_minutes, 210);
}
