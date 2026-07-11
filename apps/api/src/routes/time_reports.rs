//! Flexible time report for a project.
//!
//!   GET /projects/:key/time-report?from=&to=&sprint_id=&format=csv
//!
//! Aggregates closed time logs across an arbitrary date range and/or a sprint,
//! grouped by user / task / sprint, with totals + billable + pay, plus a CSV
//! export (`format=csv`). Permission mirrors the timesheet rules: admins and
//! project leads see the whole team; everyone else sees only their own logs.
//! The fetch + aggregation live in `domain::time_report`; this route just
//! resolves scope/filters and shapes the response.

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        permissions::{can, Action, ProjectRole, Role as GlobalRole},
        projects as project_ctx, time_report,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/projects/:key/time-report", get(time_report))
}

#[derive(Debug, Deserialize)]
struct ReportQuery {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    sprint_id: Option<Uuid>,
    format: Option<String>,
}

async fn time_report(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Query(q): Query<ReportQuery>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    // Team scope (all members' logs) for admins + project leads; otherwise the
    // caller only ever sees their own time.
    let team = user.role == GlobalRole::Admin || ctx.actor_role == Some(ProjectRole::Lead);
    let self_filter: Option<Uuid> = if team { None } else { Some(user.id) };

    // A sprint filter must name a sprint that belongs to this project.
    let sprint_name: Option<String> = if let Some(sid) = q.sprint_id {
        let name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sprints WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL",
        )
        .bind(sid)
        .bind(ctx.id)
        .fetch_optional(&state.db)
        .await?;
        Some(name.ok_or(AppError::NotFound)?)
    } else {
        None
    };

    // Date window. With neither a range nor a sprint, default to the last 30
    // days so a stray request can't scan a project's whole history.
    let (from, to) = if q.from.is_none() && q.to.is_none() && q.sprint_id.is_none() {
        let today = Utc::now().date_naive();
        (Some(today - chrono::Duration::days(30)), Some(today))
    } else {
        (q.from, q.to)
    };
    if let (Some(f), Some(t)) = (from, to) {
        if t < f {
            return Err(AppError::BadRequest("`to` is before `from`".into()));
        }
    }
    let start_ts = from.map(|d| Utc.from_utc_datetime(&NaiveDateTime::new(d, NaiveTime::MIN)));
    // `to` is inclusive of the whole day → scan up to the next midnight.
    let end_ts = to.map(|d| {
        Utc.from_utc_datetime(&NaiveDateTime::new(
            d + chrono::Duration::days(1),
            NaiveTime::MIN,
        ))
    });

    let logs = time_report::fetch_logs(
        &state.db,
        ctx.id,
        q.sprint_id,
        start_ts,
        end_ts,
        self_filter,
    )
    .await?;
    let data = time_report::aggregate(&logs);

    if q.format.as_deref() == Some("csv") {
        let csv = time_report::to_csv(&logs, data.total_minutes);
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/csv; charset=utf-8"),
        );
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!(
                "attachment; filename=\"time-report-{}.csv\"",
                ctx.key
            ))
            .unwrap(),
        );
        return Ok((StatusCode::OK, h, csv).into_response());
    }

    let project_name: String = sqlx::query_scalar("SELECT name FROM projects WHERE id = $1")
        .bind(ctx.id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(json!({
        "project_key": ctx.key,
        "project_name": project_name,
        "from": from,
        "to": to,
        "sprint_id": q.sprint_id,
        "sprint_name": sprint_name,
        "scope": if team { "team" } else { "self" },
        "total_minutes": data.total_minutes,
        "billable_minutes": data.billable_minutes,
        "total_pay_cents": data.total_pay_cents,
        "currency": data.currency,
        "by_user": data.by_user,
        "by_task": data.by_task,
        "by_sprint": data.by_sprint,
    }))
    .into_response())
}
