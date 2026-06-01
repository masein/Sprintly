//! Timesheet endpoints.
//!
//!   GET    /me/timesheets/current                         — this week's view (computed live)
//!   GET    /me/timesheets/:period_start                   — a specific past week
//!   GET    /me/timesheets                                 — submitted+approved history
//!   POST   /me/timesheets/:period_start/submit            — open → submitted
//!   POST   /timesheets/:user_id/:period_start/approve     — submitted → approved
//!   POST   /timesheets/:user_id/:period_start/mark-paid   — approved → paid
//!   GET    /timesheets/:user_id/:period_start.csv         — CSV export of approved weeks
//!
//! Submit snapshots totals + pay; approval doesn't recompute (a later edit
//! to a log shouldn't shift an approved week's pay).

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::{permissions::Role as GlobalRole, timesheets as ts},
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/timesheets/current", get(my_current))
        .route("/me/timesheets/:period_start", get(my_specific))
        .route("/me/timesheets", get(my_history))
        .route("/me/timesheets/:period_start/submit", post(submit))
        .route("/timesheets/:user_id/:period_start/approve", post(approve))
        .route(
            "/timesheets/:user_id/:period_start/mark-paid",
            post(mark_paid),
        )
        // `:period_start` carries an optional `.csv` suffix — matchit can't put
        // a literal after a param segment, so the handler splits it off.
        .route("/timesheets/:user_id/:period_start", get(csv_export))
        .route("/timesheets/pending", get(pending_approvals))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TimesheetView {
    pub user_id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub status: String,
    pub total_minutes: i64,
    pub billable_minutes: i64,
    pub total_pay_cents: i64,
    pub currency: String,
    pub days: Vec<DayBucket>,
    pub by_task: Vec<TaskBucket>,
}

#[derive(Debug, Serialize)]
pub struct DayBucket {
    pub date: NaiveDate,
    pub total_minutes: i64,
    pub billable_minutes: i64,
}

#[derive(Debug, Serialize)]
pub struct TaskBucket {
    pub task_key: String,
    pub project_key: String,
    pub task_title: String,
    pub total_minutes: i64,
    pub billable_minutes: i64,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn my_current(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let (monday, _) = ts::week_bounds(Utc::now().date_naive());
    let view = compute_view(&state.db, user.id, monday).await?;
    Ok(Json(view))
}

async fn my_specific(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(period_start): Path<NaiveDate>,
) -> AppResult<impl IntoResponse> {
    let view = compute_view(&state.db, user.id, period_start).await?;
    Ok(Json(view))
}

async fn my_history(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    let rows = sqlx::query!(
        r#"
        SELECT period_start    AS "period_start!: NaiveDate",
               period_end      AS "period_end!: NaiveDate",
               status          AS "status!: String",
               total_minutes   AS "total_minutes!: i32",
               billable_minutes AS "billable_minutes!: i32",
               total_pay_cents AS "total_pay_cents!: i64",
               currency        AS "currency!: String"
        FROM   timesheets
        WHERE  user_id = $1
        ORDER  BY period_start DESC
        LIMIT  104
        "#,
        user.id
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(serde_json::json!({
        "items": rows.iter().map(|r| serde_json::json!({
            "period_start": r.period_start,
            "period_end": r.period_end,
            "status": r.status,
            "total_minutes": r.total_minutes,
            "billable_minutes": r.billable_minutes,
            "total_pay_cents": r.total_pay_cents,
            "currency": r.currency,
        })).collect::<Vec<_>>()
    })))
}

async fn submit(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(period_start): Path<NaiveDate>,
) -> AppResult<impl IntoResponse> {
    let view = compute_view(&state.db, user.id, period_start).await?;
    if view.total_minutes == 0 {
        return Err(AppError::Conflict(
            "nothing to submit — log time first".into(),
        ));
    }
    let row: Option<(Uuid, String, NaiveDate)> = sqlx::query_as(
        r#"
        SELECT id, status, period_start
        FROM   timesheets
        WHERE  user_id = $1 AND period_start = $2
        "#,
    )
    .bind(user.id)
    .bind(period_start)
    .fetch_optional(&state.db)
    .await?;

    // Look up the user's pay rate & currency once.
    let rate: Option<(Option<i64>, String)> =
        sqlx::query_as("SELECT hourly_rate_cents, currency FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?;
    let (rate_cents, currency) = match rate {
        Some((r, c)) => (r, c),
        None => (None, "USD".into()),
    };
    let pay = ts::pay_cents(view.billable_minutes, rate_cents);

    match row {
        None => {
            sqlx::query(
                r#"
                INSERT INTO timesheets
                    (id, user_id, period_start, period_end, status, submitted_at,
                     total_minutes, billable_minutes, total_pay_cents, currency)
                VALUES
                    ($1, $2, $3, $4, 'submitted', now(),
                     $5, $6, $7, $8)
                "#,
            )
            .bind(Uuid::now_v7())
            .bind(user.id)
            .bind(view.period_start)
            .bind(view.period_end)
            .bind(view.total_minutes as i32)
            .bind(view.billable_minutes as i32)
            .bind(pay)
            .bind(&currency)
            .execute(&state.db)
            .await?;
        }
        Some((_, status, _)) if status == "open" => {
            sqlx::query(
                r#"
                UPDATE timesheets SET
                    status = 'submitted',
                    submitted_at = now(),
                    total_minutes = $2,
                    billable_minutes = $3,
                    total_pay_cents = $4,
                    currency = $5
                WHERE user_id = $1 AND period_start = $6
                "#,
            )
            .bind(user.id)
            .bind(view.total_minutes as i32)
            .bind(view.billable_minutes as i32)
            .bind(pay)
            .bind(&currency)
            .bind(period_start)
            .execute(&state.db)
            .await?;
        }
        Some((_, status, _)) => {
            return Err(AppError::Conflict(format!("timesheet is already {status}")));
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn approve(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((target_user, period_start)): Path<(Uuid, NaiveDate)>,
) -> AppResult<impl IntoResponse> {
    require_can_approve(&state.db, &user, target_user).await?;
    let n = sqlx::query(
        r#"
        UPDATE timesheets SET
            status = 'approved',
            approved_at = now(),
            approver_id = $1
        WHERE user_id = $2 AND period_start = $3 AND status = 'submitted'
        "#,
    )
    .bind(user.id)
    .bind(target_user)
    .bind(period_start)
    .execute(&state.db)
    .await?;
    if n.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "timesheet not in submitted state".into(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_paid(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((target_user, period_start)): Path<(Uuid, NaiveDate)>,
) -> AppResult<impl IntoResponse> {
    // Admin only — payroll is admin-scoped.
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    let n = sqlx::query(
        r#"
        UPDATE timesheets SET status = 'paid', paid_at = now()
        WHERE user_id = $1 AND period_start = $2 AND status = 'approved'
        "#,
    )
    .bind(target_user)
    .bind(period_start)
    .execute(&state.db)
    .await?;
    if n.rows_affected() == 0 {
        return Err(AppError::Conflict("not in approved state".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn csv_export(
    State(state): State<AppState>,
    user: CurrentUser,
    // The export URL is `.../:period_start.csv`; strip the suffix before parsing.
    Path((target_user, period_raw)): Path<(Uuid, String)>,
) -> AppResult<impl IntoResponse> {
    let period_start: NaiveDate = period_raw
        .strip_suffix(".csv")
        .unwrap_or(&period_raw)
        .parse()
        .map_err(|_| AppError::BadRequest("invalid period_start".into()))?;
    if target_user != user.id {
        require_can_approve(&state.db, &user, target_user).await?;
    }
    let view = compute_view(&state.db, target_user, period_start).await?;

    let mut csv = String::new();
    csv.push_str("date,task_key,project_key,task_title,billable,minutes\n");
    let logs = sqlx::query!(
        r#"
        SELECT tl.started_at      AS "started_at!: DateTime<Utc>",
               tl.duration_minutes,
               tl.billable        AS "billable!: bool",
               t.key              AS "task_key!: String",
               p.key              AS "project_key!: String",
               t.title            AS "title!: String"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        JOIN   projects p ON p.id = t.project_id
        WHERE  tl.user_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2
          AND  tl.started_at <  $3
        ORDER  BY tl.started_at ASC
        "#,
        target_user,
        chrono::Utc.from_utc_datetime(&NaiveDateTime::new(view.period_start, NaiveTime::MIN)),
        chrono::Utc.from_utc_datetime(&NaiveDateTime::new(
            view.period_end + chrono::Duration::days(1),
            NaiveTime::MIN
        )),
    )
    .fetch_all(&state.db)
    .await?;

    for row in logs {
        let date = row.started_at.date_naive();
        let mins = row.duration_minutes.unwrap_or(0);
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            date,
            row.task_key,
            row.project_key,
            csv_escape(&row.title),
            row.billable,
            mins
        ));
    }
    csv.push_str(&format!("\nTOTAL,,,,,{}\n", view.total_minutes));

    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"timesheet-{target_user}-{period_start}.csv\""
        ))
        .unwrap(),
    );
    Ok((StatusCode::OK, h, csv))
}

// Shared row shape for the two query branches below. Two `query!` calls
// produce distinct anonymous record types even with identical columns, so we
// map both into this named struct via `query_as!`.
struct PendingRow {
    user_id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
    total_minutes: i32,
    billable_minutes: i32,
    total_pay_cents: i64,
    currency: String,
    handle: String,
    display_name: String,
}

async fn pending_approvals(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    // Admins see every submitted week. Anyone else sees submissions from
    // users who logged time on a project they lead.
    let rows = if user.role == GlobalRole::Admin {
        sqlx::query_as!(
            PendingRow,
            r#"
            SELECT ts.user_id        AS "user_id!: Uuid",
                   ts.period_start   AS "period_start!: NaiveDate",
                   ts.period_end     AS "period_end!: NaiveDate",
                   ts.total_minutes  AS "total_minutes!: i32",
                   ts.billable_minutes AS "billable_minutes!: i32",
                   ts.total_pay_cents AS "total_pay_cents!: i64",
                   ts.currency       AS "currency!: String",
                   u.handle          AS "handle!: String",
                   u.display_name    AS "display_name!: String"
            FROM   timesheets ts
            JOIN   users u ON u.id = ts.user_id
            WHERE  ts.status = 'submitted'
            ORDER  BY ts.submitted_at ASC
            "#,
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as!(
            PendingRow,
            r#"
            SELECT DISTINCT
                   ts.user_id        AS "user_id!: Uuid",
                   ts.period_start   AS "period_start!: NaiveDate",
                   ts.period_end     AS "period_end!: NaiveDate",
                   ts.total_minutes  AS "total_minutes!: i32",
                   ts.billable_minutes AS "billable_minutes!: i32",
                   ts.total_pay_cents AS "total_pay_cents!: i64",
                   ts.currency       AS "currency!: String",
                   u.handle          AS "handle!: String",
                   u.display_name    AS "display_name!: String"
            FROM   timesheets ts
            JOIN   users u ON u.id = ts.user_id
            JOIN   time_logs tl ON tl.user_id = ts.user_id
                 AND tl.started_at >= ts.period_start::timestamptz
                 AND tl.started_at <  (ts.period_end + INTERVAL '1 day')::timestamptz
            JOIN   tasks t ON t.id = tl.task_id
            JOIN   project_members pm ON pm.project_id = t.project_id
                 AND pm.user_id = $1 AND pm.role = 'lead'
            WHERE  ts.status = 'submitted'
            ORDER  BY ts.period_start ASC
            "#,
            user.id,
        )
        .fetch_all(&state.db)
        .await?
    };
    let items: Vec<_> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "user_id": r.user_id,
                "handle": r.handle,
                "display_name": r.display_name,
                "period_start": r.period_start,
                "period_end": r.period_end,
                "total_minutes": r.total_minutes,
                "billable_minutes": r.billable_minutes,
                "total_pay_cents": r.total_pay_cents,
                "currency": r.currency,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

// ─── helpers ────────────────────────────────────────────────────────────────

async fn compute_view(
    db: &PgPool,
    user_id: Uuid,
    period_start: NaiveDate,
) -> AppResult<TimesheetView> {
    let (monday, sunday) = ts::week_bounds(period_start);
    if monday != period_start {
        return Err(AppError::BadRequest("period_start must be a Monday".into()));
    }

    use chrono::TimeZone as _;
    let start_ts = Utc.from_utc_datetime(&NaiveDateTime::new(monday, NaiveTime::MIN));
    let end_ts = Utc.from_utc_datetime(&NaiveDateTime::new(
        sunday + chrono::Duration::days(1),
        NaiveTime::MIN,
    ));

    // Snapshot if there's already a row for this week.
    let stored = sqlx::query!(
        r#"
        SELECT status          AS "status!: String",
               total_minutes   AS "total_minutes!: i32",
               billable_minutes AS "billable_minutes!: i32",
               total_pay_cents AS "total_pay_cents!: i64",
               currency        AS "currency!: String"
        FROM   timesheets
        WHERE  user_id = $1 AND period_start = $2
        "#,
        user_id,
        period_start
    )
    .fetch_optional(db)
    .await?;

    let logs = sqlx::query!(
        r#"
        SELECT tl.started_at      AS "started_at!: DateTime<Utc>",
               tl.duration_minutes,
               tl.billable        AS "billable!: bool",
               t.key              AS "task_key!: String",
               p.key              AS "project_key!: String",
               t.title            AS "task_title!: String"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        JOIN   projects p ON p.id = t.project_id
        WHERE  tl.user_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2
          AND  tl.started_at <  $3
        ORDER  BY tl.started_at ASC
        "#,
        user_id,
        start_ts,
        end_ts,
    )
    .fetch_all(db)
    .await?;

    let mut day_totals: BTreeMap<NaiveDate, (i64, i64)> = BTreeMap::new();
    let mut task_totals: BTreeMap<(String, String, String), (i64, i64)> = BTreeMap::new();
    let mut totals = ts::Totals::default();

    for row in &logs {
        let mins = row.duration_minutes.unwrap_or(0) as i64;
        let d = row.started_at.date_naive();
        let entry = day_totals.entry(d).or_insert((0, 0));
        entry.0 += mins;
        if row.billable {
            entry.1 += mins;
        }
        let tk = (
            row.task_key.clone(),
            row.project_key.clone(),
            row.task_title.clone(),
        );
        let te = task_totals.entry(tk).or_insert((0, 0));
        te.0 += mins;
        if row.billable {
            te.1 += mins;
        }
        totals.add(mins, row.billable);
    }

    // Always emit 7 day buckets (Mon → Sun) so the UI grid stays uniform.
    let days: Vec<DayBucket> = (0..7)
        .map(|i| {
            let d = monday + chrono::Duration::days(i);
            let (t, b) = day_totals.get(&d).copied().unwrap_or((0, 0));
            DayBucket {
                date: d,
                total_minutes: t,
                billable_minutes: b,
            }
        })
        .collect();

    let by_task: Vec<TaskBucket> = task_totals
        .into_iter()
        .map(|((task_key, project_key, task_title), (t, b))| TaskBucket {
            task_key,
            project_key,
            task_title,
            total_minutes: t,
            billable_minutes: b,
        })
        .collect();

    let (status, total_min, billable_min, pay_cents, currency) = match &stored {
        Some(s) if s.status != "open" => (
            s.status.clone(),
            s.total_minutes as i64,
            s.billable_minutes as i64,
            s.total_pay_cents,
            s.currency.clone(),
        ),
        _ => {
            // Compute live pay from current rate.
            let rate: Option<(Option<i64>, String)> =
                sqlx::query_as("SELECT hourly_rate_cents, currency FROM users WHERE id = $1")
                    .bind(user_id)
                    .fetch_optional(db)
                    .await?;
            let (rate_cents, cur) = match rate {
                Some((r, c)) => (r, c),
                None => (None, "USD".into()),
            };
            (
                "open".to_string(),
                totals.total_minutes,
                totals.billable_minutes,
                ts::pay_cents(totals.billable_minutes, rate_cents),
                cur,
            )
        }
    };

    Ok(TimesheetView {
        user_id,
        period_start: monday,
        period_end: sunday,
        status,
        total_minutes: total_min,
        billable_minutes: billable_min,
        total_pay_cents: pay_cents,
        currency,
        days,
        by_task,
    })
}

/// True if `actor` is allowed to approve `target`'s timesheets:
///   • global admin, OR
///   • project lead of any project where `target` logged time.
async fn require_can_approve(db: &PgPool, actor: &CurrentUser, target: Uuid) -> AppResult<()> {
    if actor.role == GlobalRole::Admin {
        return Ok(());
    }
    let ok: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM   time_logs tl
            JOIN   tasks t ON t.id = tl.task_id
            JOIN   project_members pm
                   ON pm.project_id = t.project_id
                   AND pm.user_id = $1
                   AND pm.role = 'lead'
            WHERE  tl.user_id = $2 AND tl.deleted_at IS NULL
        )
        "#,
    )
    .bind(actor.id)
    .bind(target)
    .fetch_one(db)
    .await?;
    if ok {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// chrono::TimeZone is referenced in compute_view; surface it once at top.
use chrono::TimeZone as _;
