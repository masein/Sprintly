//! Flexible time report (M4+): closed time logs for a project, over an
//! arbitrary date range and/or a single sprint, folded into totals and
//! per-user / per-task / per-sprint breakdowns, with a matching CSV export.
//!
//! Pay is money, so it's BIGINT cents throughout and computed per user from
//! that user's own rate — never averaged, never floated. The SQL fetch lives
//! here (next to the aggregation it feeds) so it's testable without the HTTP
//! stack; the route stays thin.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::timesheets::pay_cents, AppResult};

/// One closed time-log, denormalised with everything the report needs.
/// `started_at` is used only for the CSV row export, not the aggregation.
#[derive(Debug, Clone)]
pub struct FetchedLog {
    pub started_at: DateTime<Utc>,
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub rate_cents: Option<i64>,
    pub currency: String,
    pub task_key: String,
    pub task_title: String,
    pub sprint_id: Option<Uuid>,
    pub sprint_name: Option<String>,
    pub minutes: i64,
    pub billable: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct UserBucket {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub total_minutes: i64,
    pub billable_minutes: i64,
    pub pay_cents: i64,
    pub currency: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TaskBucket {
    pub task_key: String,
    pub task_title: String,
    pub total_minutes: i64,
    pub billable_minutes: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SprintBucket {
    pub sprint_id: Option<Uuid>,
    pub sprint_name: Option<String>,
    pub total_minutes: i64,
    pub billable_minutes: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TimeReportData {
    pub total_minutes: i64,
    pub billable_minutes: i64,
    pub total_pay_cents: i64,
    /// The single currency across all users, or `"MIXED"` when they differ (so
    /// the UI knows not to trust the summed `total_pay_cents` as one currency).
    pub currency: String,
    pub by_user: Vec<UserBucket>,
    pub by_task: Vec<TaskBucket>,
    pub by_sprint: Vec<SprintBucket>,
}

/// Fetch the closed logs for a project matching the optional filters. Every
/// filter is applied only when its bind is non-NULL, so one static query serves
/// range-only, sprint-only, and combined requests. `self_filter = Some(uid)`
/// restricts to one user's logs (the "members see only themselves" scope).
pub async fn fetch_logs(
    db: &PgPool,
    project_id: Uuid,
    sprint_id: Option<Uuid>,
    start_ts: Option<DateTime<Utc>>,
    end_ts: Option<DateTime<Utc>>,
    self_filter: Option<Uuid>,
) -> AppResult<Vec<FetchedLog>> {
    let recs = sqlx::query!(
        r#"
        SELECT tl.started_at    AS "started_at!: DateTime<Utc>",
               tl.duration_minutes,
               tl.billable      AS "billable!: bool",
               u.id             AS "user_id!: Uuid",
               u.handle         AS "handle!: String",
               u.display_name   AS "display_name!: String",
               u.hourly_rate_cents,
               u.currency       AS "currency!: String",
               t.key            AS "task_key!: String",
               t.title          AS "task_title!: String",
               s.id             AS "sprint_id?: Uuid",
               s.name           AS "sprint_name?: String"
        FROM   time_logs tl
        JOIN   tasks t    ON t.id = tl.task_id
        JOIN   users u    ON u.id = tl.user_id
        LEFT   JOIN sprints s ON s.id = t.sprint_id AND s.deleted_at IS NULL
        WHERE  t.project_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  ($2::uuid IS NULL OR t.sprint_id = $2)
          AND  ($3::timestamptz IS NULL OR tl.started_at >= $3)
          AND  ($4::timestamptz IS NULL OR tl.started_at <  $4)
          AND  ($5::uuid IS NULL OR tl.user_id = $5)
        ORDER  BY tl.started_at ASC
        "#,
        project_id,
        sprint_id,
        start_ts,
        end_ts,
        self_filter,
    )
    .fetch_all(db)
    .await?;

    Ok(recs
        .into_iter()
        .map(|r| FetchedLog {
            started_at: r.started_at,
            user_id: r.user_id,
            handle: r.handle,
            display_name: r.display_name,
            rate_cents: r.hourly_rate_cents,
            currency: r.currency,
            task_key: r.task_key,
            task_title: r.task_title,
            sprint_id: r.sprint_id,
            sprint_name: r.sprint_name,
            minutes: r.duration_minutes.unwrap_or(0) as i64,
            billable: r.billable,
        })
        .collect())
}

#[derive(Default)]
struct UserAcc {
    handle: String,
    display_name: String,
    rate_cents: Option<i64>,
    currency: String,
    total: i64,
    billable: i64,
}

/// Fold logs into the report. Buckets are sorted by total time descending (ties
/// broken by name/key) so the biggest contributors read first; the "no sprint"
/// bucket, if any, sorts last.
pub fn aggregate(rows: &[FetchedLog]) -> TimeReportData {
    let mut users: HashMap<Uuid, UserAcc> = HashMap::new();
    let mut tasks: HashMap<String, (String, i64, i64)> = HashMap::new();
    let mut sprints: HashMap<Option<Uuid>, (Option<String>, i64, i64)> = HashMap::new();
    let mut total = 0i64;
    let mut billable = 0i64;

    for r in rows {
        if r.minutes <= 0 {
            continue;
        }
        total += r.minutes;
        if r.billable {
            billable += r.minutes;
        }

        let u = users.entry(r.user_id).or_insert_with(|| UserAcc {
            handle: r.handle.clone(),
            display_name: r.display_name.clone(),
            rate_cents: r.rate_cents,
            currency: r.currency.clone(),
            ..Default::default()
        });
        u.total += r.minutes;
        if r.billable {
            u.billable += r.minutes;
        }

        let t = tasks
            .entry(r.task_key.clone())
            .or_insert_with(|| (r.task_title.clone(), 0, 0));
        t.1 += r.minutes;
        if r.billable {
            t.2 += r.minutes;
        }

        let s = sprints
            .entry(r.sprint_id)
            .or_insert_with(|| (r.sprint_name.clone(), 0, 0));
        s.1 += r.minutes;
        if r.billable {
            s.2 += r.minutes;
        }
    }

    let mut by_user: Vec<UserBucket> = users
        .into_iter()
        .map(|(user_id, a)| UserBucket {
            user_id,
            handle: a.handle,
            display_name: a.display_name,
            total_minutes: a.total,
            billable_minutes: a.billable,
            pay_cents: pay_cents(a.billable, a.rate_cents),
            currency: a.currency,
        })
        .collect();
    by_user.sort_by(|a, b| {
        b.total_minutes
            .cmp(&a.total_minutes)
            .then_with(|| a.handle.cmp(&b.handle))
    });

    let mut by_task: Vec<TaskBucket> = tasks
        .into_iter()
        .map(|(task_key, (task_title, t, b))| TaskBucket {
            task_key,
            task_title,
            total_minutes: t,
            billable_minutes: b,
        })
        .collect();
    by_task.sort_by(|a, b| {
        b.total_minutes
            .cmp(&a.total_minutes)
            .then_with(|| a.task_key.cmp(&b.task_key))
    });

    let mut by_sprint: Vec<SprintBucket> = sprints
        .into_iter()
        .map(|(sprint_id, (sprint_name, t, b))| SprintBucket {
            sprint_id,
            sprint_name,
            total_minutes: t,
            billable_minutes: b,
        })
        .collect();
    by_sprint.sort_by(|a, b| {
        // Real sprints first (by time desc); the "no sprint" bucket sinks last.
        b.sprint_id
            .is_some()
            .cmp(&a.sprint_id.is_some())
            .then_with(|| b.total_minutes.cmp(&a.total_minutes))
    });

    // Single currency, or MIXED so the UI won't add apples to oranges.
    let mut currency = String::new();
    let mut mixed = false;
    for u in &by_user {
        if currency.is_empty() {
            currency = u.currency.clone();
        } else if currency != u.currency {
            mixed = true;
        }
    }
    let currency = if mixed {
        "MIXED".to_string()
    } else if currency.is_empty() {
        "USD".to_string()
    } else {
        currency
    };
    let total_pay_cents = by_user.iter().map(|u| u.pay_cents).sum();

    TimeReportData {
        total_minutes: total,
        billable_minutes: billable,
        total_pay_cents,
        currency,
        by_user,
        by_task,
        by_sprint,
    }
}

/// A task's tracked time, with direct subtask logs rolled up into the total.
/// One level deep on purpose — that's as deep as the subtasks panel creates,
/// and it keeps the sum cheap and predictable.
#[derive(Debug, Serialize)]
pub struct TaskTimeSummary {
    pub own_minutes: i64,
    pub subtask_minutes: i64,
    pub total_minutes: i64,
}

pub async fn task_time_summary(pool: &PgPool, task_id: Uuid) -> AppResult<TaskTimeSummary> {
    let row = sqlx::query!(
        r#"
        SELECT COALESCE(SUM(tl.duration_minutes) FILTER (WHERE tl.task_id = $1), 0)::bigint
                   AS "own_minutes!: i64",
               COALESCE(SUM(tl.duration_minutes) FILTER (WHERE tl.task_id <> $1), 0)::bigint
                   AS "subtask_minutes!: i64"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        WHERE  (t.id = $1 OR t.parent_task_id = $1)
          AND  t.deleted_at IS NULL
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
        "#,
        task_id
    )
    .fetch_one(pool)
    .await?;
    Ok(TaskTimeSummary {
        own_minutes: row.own_minutes,
        subtask_minutes: row.subtask_minutes,
        total_minutes: row.own_minutes + row.subtask_minutes,
    })
}

/// CSV export: one row per log (chronological), then a TOTAL line. The rows sum
/// to the same `total_minutes` the JSON report reports.
pub fn to_csv(logs: &[FetchedLog], total_minutes: i64) -> String {
    let mut csv = String::from("date,user,task_key,task_title,sprint,billable,minutes\n");
    for r in logs {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            r.started_at.date_naive(),
            csv_escape(&r.handle),
            r.task_key,
            csv_escape(&r.task_title),
            csv_escape(r.sprint_name.as_deref().unwrap_or("")),
            r.billable,
            r.minutes,
        ));
    }
    csv.push_str(&format!("\nTOTAL,,,,,,{total_minutes}\n"));
    csv
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        user: Uuid,
        handle: &str,
        task: &str,
        sprint: Option<(Uuid, &str)>,
        mins: i64,
        billable: bool,
        rate: Option<i64>,
    ) -> FetchedLog {
        FetchedLog {
            started_at: chrono::DateTime::UNIX_EPOCH,
            user_id: user,
            handle: handle.into(),
            display_name: handle.into(),
            rate_cents: rate,
            currency: "USD".into(),
            task_key: task.into(),
            task_title: format!("{task} title"),
            sprint_id: sprint.map(|s| s.0),
            sprint_name: sprint.map(|s| s.1.into()),
            minutes: mins,
            billable,
        }
    }

    #[test]
    fn totals_and_breakdowns() {
        let u1 = Uuid::now_v7();
        let u2 = Uuid::now_v7();
        let sp = Uuid::now_v7();
        let rows = vec![
            row(
                u1,
                "ann",
                "P-1",
                Some((sp, "Sprint 1")),
                60,
                true,
                Some(6000),
            ),
            row(u1, "ann", "P-2", None, 30, false, Some(6000)),
            row(
                u2,
                "bob",
                "P-1",
                Some((sp, "Sprint 1")),
                120,
                true,
                Some(3000),
            ),
        ];
        let r = aggregate(&rows);
        assert_eq!(r.total_minutes, 210);
        assert_eq!(r.billable_minutes, 180);
        // ann: 60 billable @ 6000/hr = 6000c; bob: 120 billable @ 3000/hr = 6000c.
        assert_eq!(r.total_pay_cents, 12000);
        assert_eq!(r.currency, "USD");
        // by_user sorted by total desc: bob (120) before ann (90).
        assert_eq!(r.by_user[0].handle, "bob");
        assert_eq!(r.by_user[0].total_minutes, 120);
        assert_eq!(r.by_user[1].handle, "ann");
        assert_eq!(r.by_user[1].total_minutes, 90);
        // by_task: P-1 (180) before P-2 (30).
        assert_eq!(r.by_task[0].task_key, "P-1");
        assert_eq!(r.by_task[0].total_minutes, 180);
        assert_eq!(r.by_task[0].billable_minutes, 180);
        // by_sprint: Sprint 1 (180) before the no-sprint bucket (30).
        assert_eq!(r.by_sprint[0].sprint_id, Some(sp));
        assert_eq!(r.by_sprint[0].total_minutes, 180);
        assert_eq!(r.by_sprint[1].sprint_id, None);
        assert_eq!(r.by_sprint[1].total_minutes, 30);
    }

    #[test]
    fn mixed_currency_is_flagged() {
        let u1 = Uuid::now_v7();
        let u2 = Uuid::now_v7();
        let mut a = row(u1, "ann", "P-1", None, 60, true, Some(6000));
        a.currency = "USD".into();
        let mut b = row(u2, "bob", "P-1", None, 60, true, Some(6000));
        b.currency = "EUR".into();
        let r = aggregate(&[a, b]);
        assert_eq!(r.currency, "MIXED");
    }

    #[test]
    fn skips_zero_and_negative_minutes() {
        let u1 = Uuid::now_v7();
        let rows = vec![
            row(u1, "ann", "P-1", None, 0, true, Some(6000)),
            row(u1, "ann", "P-1", None, -5, true, Some(6000)),
            row(u1, "ann", "P-1", None, 45, true, Some(6000)),
        ];
        let r = aggregate(&rows);
        assert_eq!(r.total_minutes, 45);
        assert_eq!(r.by_user.len(), 1);
    }

    #[test]
    fn csv_has_header_rows_and_total() {
        let u1 = Uuid::now_v7();
        let rows = vec![row(
            u1,
            "ann",
            "P-1",
            Some((Uuid::now_v7(), "Sprint 1")),
            45,
            true,
            Some(6000),
        )];
        let data = aggregate(&rows);
        let csv = to_csv(&rows, data.total_minutes);
        assert!(csv.starts_with("date,user,task_key,task_title,sprint,billable,minutes\n"));
        assert!(csv.contains(",ann,P-1,P-1 title,Sprint 1,true,45\n"));
        assert!(csv.trim_end().ends_with("TOTAL,,,,,,45"));
    }
}
