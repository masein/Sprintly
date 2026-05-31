//! Payroll endpoints — admin surface for monthly summaries + per-project budgets.
//!
//!   GET    /payroll/:year/:month                   — admin: all users for the month
//!   GET    /payroll/:user_id/:year/:month          — single-user detail
//!   GET    /payroll/:year/:month.csv               — same data, CSV
//!   GET    /payroll/:user_id/:year/:month.pdf      — single-user one-page PDF
//!   POST   /payroll/:user_id/:year/:month/mark-paid
//!   POST   /payroll/:user_id/:year/:month/reopen   — back to 'open'
//!
//!   PATCH  /projects/:key/budget                   — set budget_cents + currency
//!   GET    /projects/:key/burn                     — current-month spend vs budget

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    domain::{
        payroll as payroll_math,
        permissions::{can, Action, Role as GlobalRole},
        projects as project_ctx,
    },
    infra::{pdf::PdfBuilder, AppState},
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/payroll/:year/:month", get(month_overview))
        .route("/payroll/:year/:month.csv", get(month_csv))
        .route("/payroll/:user_id/:year/:month", get(user_month))
        .route("/payroll/:user_id/:year/:month.pdf", get(user_month_pdf))
        .route("/payroll/:user_id/:year/:month/mark-paid", post(mark_paid))
        .route("/payroll/:user_id/:year/:month/reopen", post(reopen))
        .route("/projects/:key/budget", axum::routing::patch(set_budget))
        .route("/projects/:key/burn", get(burn))
}

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct UserMonthSummary {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub total_minutes: i64,
    pub billable_minutes: i64,
    pub total_pay_cents: i64,
    pub currency: String,
    pub status: String,
    pub paid_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct MonthOverview {
    pub year: i32,
    pub month: u32,
    pub users: Vec<UserMonthSummary>,
    pub grand_total_pay_cents: i64,
    pub currency: String,
}

#[derive(Debug, Serialize)]
pub struct UserMonthDetail {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub year: i32,
    pub month: u32,
    pub total_minutes: i64,
    pub billable_minutes: i64,
    pub total_pay_cents: i64,
    pub currency: String,
    pub status: String,
    pub paid_at: Option<chrono::DateTime<Utc>>,
    pub by_project: Vec<ProjectLine>,
}

#[derive(Debug, Serialize)]
pub struct ProjectLine {
    pub project_key: String,
    pub project_name: String,
    pub total_minutes: i64,
    pub billable_minutes: i64,
}

#[derive(Debug, Deserialize)]
pub struct SetBudgetReq {
    /// Pass `null` to clear.
    pub budget_cents: Option<i64>,
    pub budget_currency: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BurnDto {
    pub spent_cents: i64,
    pub budget_cents: Option<i64>,
    pub currency: String,
    pub elapsed_fraction: f64,
    pub status: String,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn month_overview(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((year, month)): Path<(i32, u32)>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    let (first, last) = bounds_or_400(year, month)?;
    let users = aggregate_users(&state.db, first, last).await?;
    let stamped = decorate_with_period_status(&state.db, year, month, users).await?;
    let grand = stamped.iter().map(|u| u.total_pay_cents).sum::<i64>();
    Ok(Json(MonthOverview {
        year,
        month,
        users: stamped,
        grand_total_pay_cents: grand,
        currency: "USD".into(),
    }))
}

async fn month_csv(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((year, month)): Path<(i32, u32)>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    let (first, last) = bounds_or_400(year, month)?;
    let users = aggregate_users(&state.db, first, last).await?;
    let stamped = decorate_with_period_status(&state.db, year, month, users).await?;
    let mut csv = String::from("handle,display_name,total_minutes,billable_minutes,total_pay_cents,currency,status\n");
    for u in &stamped {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            u.handle,
            csv_escape(&u.display_name),
            u.total_minutes,
            u.billable_minutes,
            u.total_pay_cents,
            u.currency,
            u.status,
        ));
    }
    Ok(csv_response(
        format!("payroll-{year}-{month:02}.csv"),
        csv,
    ))
}

async fn user_month(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((target_user, year, month)): Path<(Uuid, i32, u32)>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin && target_user != user.id {
        return Err(AppError::Forbidden);
    }
    let (first, last) = bounds_or_400(year, month)?;
    let detail = aggregate_user_detail(&state.db, target_user, year, month, first, last).await?;
    Ok(Json(detail))
}

async fn user_month_pdf(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((target_user, year, month)): Path<(Uuid, i32, u32)>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin && target_user != user.id {
        return Err(AppError::Forbidden);
    }
    let (first, last) = bounds_or_400(year, month)?;
    let d = aggregate_user_detail(&state.db, target_user, year, month, first, last).await?;

    // Hand-rolled PDF. Header + per-project table + total.
    let mut pdf = PdfBuilder::new();
    pdf.text_top(50.0, 60.0, 18.0, "Sprintly — Payroll Report");
    pdf.text_top(50.0, 84.0, 11.0, &format!("User: {} (@{})", d.display_name, d.handle));
    pdf.text_top(50.0, 100.0, 11.0, &format!("Period: {year}-{month:02}"));
    pdf.text_top(50.0, 116.0, 11.0, &format!("Status: {}", d.status));

    pdf.text_top(50.0, 156.0, 12.0, "Project");
    pdf.text_top(280.0, 156.0, 12.0, "Minutes");
    pdf.text_top(380.0, 156.0, 12.0, "Billable");

    let mut y = 176.0;
    for line in &d.by_project {
        pdf.text_top(50.0, y, 11.0, &format!("{} — {}", line.project_key, line.project_name));
        pdf.text_top(280.0, y, 11.0, &fmt_minutes(line.total_minutes));
        pdf.text_top(380.0, y, 11.0, &fmt_minutes(line.billable_minutes));
        y += 18.0;
    }
    y += 12.0;
    pdf.text_top(50.0, y, 12.0, &format!("Total: {}", fmt_minutes(d.total_minutes)));
    pdf.text_top(50.0, y + 18.0, 12.0,
        &format!("Pay: {} {:.2}", d.currency, (d.total_pay_cents as f64) / 100.0));
    pdf.text_top(
        50.0,
        PAGE_H_FROM_TOP_FOOTER,
        9.0,
        &format!("Generated by Sprintly · {}", Utc::now().to_rfc3339()),
    );

    let bytes = pdf.finish();
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"payroll-{handle}-{year}-{month:02}.pdf\"",
            handle = d.handle
        ))
        .unwrap(),
    );
    Ok((StatusCode::OK, h, bytes))
}

const PAGE_H_FROM_TOP_FOOTER: f32 = 760.0;

async fn mark_paid(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((target_user, year, month)): Path<(Uuid, i32, u32)>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        INSERT INTO payroll_periods (user_id, period_year, period_month, status, paid_at, paid_by)
        VALUES ($1, $2, $3, 'paid', now(), $4)
        ON CONFLICT (user_id, period_year, period_month) DO UPDATE SET
            status = 'paid',
            paid_at = COALESCE(payroll_periods.paid_at, now()),
            paid_by = COALESCE(payroll_periods.paid_by, EXCLUDED.paid_by)
        "#,
    )
    .bind(target_user)
    .bind(year as i16)
    .bind(month as i16)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reopen(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((target_user, year, month)): Path<(Uuid, i32, u32)>,
) -> AppResult<impl IntoResponse> {
    if user.role != GlobalRole::Admin {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        r#"
        UPDATE payroll_periods SET status = 'open', paid_at = NULL, paid_by = NULL
        WHERE user_id = $1 AND period_year = $2 AND period_month = $3
        "#,
    )
    .bind(target_user)
    .bind(year as i16)
    .bind(month as i16)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_budget(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<SetBudgetReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(b) = req.budget_cents {
        if b < 0 {
            return Err(AppError::Validation("budget_cents must be >= 0".into()));
        }
    }
    sqlx::query(
        r#"
        UPDATE projects SET
            budget_cents     = $2,
            budget_currency  = COALESCE($3, budget_currency)
        WHERE id = $1
        "#,
    )
    .bind(ctx.id)
    .bind(req.budget_cents)
    .bind(req.budget_currency.as_deref())
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn burn(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let proj = sqlx::query!(
        r#"
        SELECT budget_cents,
               budget_currency AS "budget_currency!: String"
        FROM   projects WHERE id = $1
        "#,
        ctx.id
    )
    .fetch_one(&state.db)
    .await?;

    let today = Utc::now().date_naive();
    let (first, last) = payroll_math::month_bounds(today.year(), today.month())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("bad month bounds")))?;
    let start_ts = Utc.from_utc_datetime(&NaiveDateTime::new(first, NaiveTime::MIN));
    let end_ts = Utc.from_utc_datetime(&NaiveDateTime::new(
        last + chrono::Duration::days(1),
        NaiveTime::MIN,
    ));

    // Spent = sum over time_logs (joined to tasks in this project) of
    // duration_minutes × user.hourly_rate_cents / 60. Compute in SQL so we
    // can do it without pulling every log into Rust memory.
    let spent_cents: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(
                 SUM(
                   COALESCE(u.hourly_rate_cents, 0)
                   * tl.duration_minutes
                   / 60
                 ),
                 0
               )::bigint
        FROM   time_logs tl
        JOIN   tasks t  ON t.id = tl.task_id
        JOIN   users u  ON u.id = tl.user_id
        WHERE  t.project_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2 AND tl.started_at < $3
          AND  tl.billable = true
        "#,
    )
    .bind(ctx.id)
    .bind(start_ts)
    .bind(end_ts)
    .fetch_one(&state.db)
    .await?;

    let elapsed = payroll_math::month_elapsed_fraction(today);
    let status =
        payroll_math::burn_status(spent_cents, proj.budget_cents, elapsed).as_str();
    Ok(Json(BurnDto {
        spent_cents,
        budget_cents: proj.budget_cents,
        currency: proj.budget_currency,
        elapsed_fraction: elapsed,
        status: status.to_string(),
    }))
}

// ─── shared aggregation ─────────────────────────────────────────────────────

async fn aggregate_users(
    db: &PgPool,
    first: NaiveDate,
    last: NaiveDate,
) -> AppResult<Vec<UserMonthSummary>> {
    let start_ts = Utc.from_utc_datetime(&NaiveDateTime::new(first, NaiveTime::MIN));
    let end_ts = Utc.from_utc_datetime(&NaiveDateTime::new(
        last + chrono::Duration::days(1),
        NaiveTime::MIN,
    ));
    let rows = sqlx::query!(
        r#"
        SELECT u.id                                AS "user_id!: Uuid",
               u.handle                            AS "handle!: String",
               u.display_name                      AS "display_name!: String",
               u.currency                          AS "currency!: String",
               COALESCE(u.hourly_rate_cents, 0)    AS "rate!: i64",
               COALESCE(SUM(tl.duration_minutes), 0)::bigint AS "total_minutes!: i64",
               COALESCE(SUM(tl.duration_minutes)
                        FILTER (WHERE tl.billable = true), 0)::bigint
                                                   AS "billable_minutes!: i64"
        FROM   users u
        LEFT JOIN time_logs tl ON tl.user_id = u.id
               AND tl.deleted_at IS NULL
               AND tl.ended_at IS NOT NULL
               AND tl.started_at >= $1 AND tl.started_at < $2
        WHERE  u.deleted_at IS NULL AND u.status = 'active'
        GROUP  BY u.id, u.handle, u.display_name, u.currency, u.hourly_rate_cents
        ORDER  BY u.handle
        "#,
        start_ts,
        end_ts
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let pay = payroll_math::pay_cents(r.billable_minutes, Some(r.rate));
            UserMonthSummary {
                user_id: r.user_id,
                handle: r.handle,
                display_name: r.display_name,
                total_minutes: r.total_minutes,
                billable_minutes: r.billable_minutes,
                total_pay_cents: pay,
                currency: r.currency,
                status: "open".into(),
                paid_at: None,
            }
        })
        .collect())
}

async fn decorate_with_period_status(
    db: &PgPool,
    year: i32,
    month: u32,
    mut users: Vec<UserMonthSummary>,
) -> AppResult<Vec<UserMonthSummary>> {
    if users.is_empty() {
        return Ok(users);
    }
    let ids: Vec<Uuid> = users.iter().map(|u| u.user_id).collect();
    let rows = sqlx::query!(
        r#"
        SELECT user_id   AS "user_id!: Uuid",
               status    AS "status!: String",
               paid_at
        FROM   payroll_periods
        WHERE  period_year = $1 AND period_month = $2 AND user_id = ANY($3)
        "#,
        year as i16,
        month as i16,
        &ids
    )
    .fetch_all(db)
    .await?;
    let mut by_user: HashMap<Uuid, (String, Option<chrono::DateTime<Utc>>)> = HashMap::new();
    for r in rows {
        by_user.insert(r.user_id, (r.status, r.paid_at));
    }
    for u in users.iter_mut() {
        if let Some((s, p)) = by_user.remove(&u.user_id) {
            u.status = s;
            u.paid_at = p;
        }
    }
    Ok(users)
}

async fn aggregate_user_detail(
    db: &PgPool,
    target: Uuid,
    year: i32,
    month: u32,
    first: NaiveDate,
    last: NaiveDate,
) -> AppResult<UserMonthDetail> {
    let start_ts = Utc.from_utc_datetime(&NaiveDateTime::new(first, NaiveTime::MIN));
    let end_ts = Utc.from_utc_datetime(&NaiveDateTime::new(
        last + chrono::Duration::days(1),
        NaiveTime::MIN,
    ));
    let header = sqlx::query!(
        r#"
        SELECT handle        AS "handle!: String",
               display_name  AS "display_name!: String",
               currency      AS "currency!: String",
               COALESCE(hourly_rate_cents, 0) AS "rate!: i64"
        FROM   users WHERE id = $1 AND deleted_at IS NULL
        "#,
        target
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    let lines = sqlx::query!(
        r#"
        SELECT p.key         AS "key!: String",
               p.name        AS "name!: String",
               COALESCE(SUM(tl.duration_minutes), 0)::bigint AS "total!: i64",
               COALESCE(SUM(tl.duration_minutes) FILTER (WHERE tl.billable = true), 0)::bigint
                              AS "billable!: i64"
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        JOIN   projects p ON p.id = t.project_id
        WHERE  tl.user_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2 AND tl.started_at < $3
        GROUP  BY p.key, p.name
        ORDER  BY "total!: i64" DESC
        "#,
        target,
        start_ts,
        end_ts
    )
    .fetch_all(db)
    .await?;

    let mut total = 0i64;
    let mut billable = 0i64;
    let by_project: Vec<ProjectLine> = lines
        .into_iter()
        .map(|r| {
            total += r.total;
            billable += r.billable;
            ProjectLine {
                project_key: r.key,
                project_name: r.name,
                total_minutes: r.total,
                billable_minutes: r.billable,
            }
        })
        .collect();
    let pay = payroll_math::pay_cents(billable, Some(header.rate));

    let period = sqlx::query!(
        r#"SELECT status AS "status!: String", paid_at
           FROM payroll_periods WHERE user_id = $1 AND period_year = $2 AND period_month = $3"#,
        target,
        year as i16,
        month as i16,
    )
    .fetch_optional(db)
    .await?;
    let (status, paid_at) = match period {
        Some(p) => (p.status, p.paid_at),
        None => ("open".into(), None),
    };

    Ok(UserMonthDetail {
        user_id: target,
        handle: header.handle,
        display_name: header.display_name,
        year,
        month,
        total_minutes: total,
        billable_minutes: billable,
        total_pay_cents: pay,
        currency: header.currency,
        status,
        paid_at,
        by_project,
    })
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn bounds_or_400(year: i32, month: u32) -> AppResult<(NaiveDate, NaiveDate)> {
    payroll_math::month_bounds(year, month)
        .ok_or_else(|| AppError::BadRequest("month must be 1..=12".into()))
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_response(filename: String, body: String) -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")).unwrap(),
    );
    (StatusCode::OK, h, body)
}

fn fmt_minutes(m: i64) -> String {
    if m < 60 {
        format!("{m}m")
    } else {
        let h = m / 60;
        let r = m % 60;
        if r == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {r}m")
        }
    }
}

