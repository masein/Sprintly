//! The team view on the project dashboard: who logged what on which day (a
//! "clockwork" table — members × days, for any range), and each member's
//! sprint KPIs (KPI request alongside QA report 6: "there is no way to see
//! others' KPIs, clockwork in the dashboard of the project").
//!
//!   GET /projects/:key/team?from=&to=&sprint_id=
//!
//! Who sees what follows the rules the time report already set: admins and
//! project leads see everyone's hours; anyone else sees their own hours and
//! a blank for the rest. The KPIs are counts of sprint tasks — who has what,
//! what's done, on time, estimated — which the board already shows every
//! member, so everyone on the project sees the whole KPI table.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        permissions::{can, Action, ProjectRole, Role as GlobalRole},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/projects/:key/team", get(team))
}

/// Longest range one request may ask for — a quarter of daily columns.
const MAX_DAYS: i64 = 92;

#[derive(Debug, Deserialize)]
struct TeamQuery {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    sprint_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct MemberKpi {
    /// Sprint tasks assigned to them.
    pub assigned: i64,
    pub completed: i64,
    /// Completed tasks that had a due date — the on-time rate's denominator.
    pub completed_with_due: i64,
    /// …of which finished on or before it.
    pub on_time: i64,
    /// Assigned tasks carrying an estimate (minutes or story points).
    pub estimated: i64,
}

#[derive(Debug, Serialize)]
struct MemberRow {
    user_id: Uuid,
    handle: String,
    display_name: String,
    avatar_url: Option<String>,
    avatar_style: Option<String>,
    avatar_seed: Option<String>,
    role: String,
    /// One entry per day in `days`; `null` when the caller may not see it.
    minutes_by_day: Option<Vec<i64>>,
    total_minutes: Option<i64>,
    kpi: MemberKpi,
}

/// Fold logs into per-user day columns. Days are UTC dates of `started_at`,
/// like the rest of the time reporting; logs outside the range are ignored.
pub fn bucket_by_day(
    from: NaiveDate,
    days: usize,
    logs: &[(Uuid, DateTime<Utc>, i64)],
) -> HashMap<Uuid, Vec<i64>> {
    let mut out: HashMap<Uuid, Vec<i64>> = HashMap::new();
    for (user, at, minutes) in logs {
        let idx = (at.date_naive() - from).num_days();
        if idx < 0 || idx as usize >= days || *minutes <= 0 {
            continue;
        }
        out.entry(*user).or_insert_with(|| vec![0; days])[idx as usize] += minutes;
    }
    out
}

/// Monday of the week `d` is in.
fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

async fn team(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Query(q): Query<TeamQuery>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let team_scope = user.role == GlobalRole::Admin || ctx.actor_role == Some(ProjectRole::Lead);

    // Range: this week (Mon–Sun) unless asked otherwise.
    let today = Utc::now().date_naive();
    let from = q.from.unwrap_or_else(|| monday_of(today));
    let to = q.to.unwrap_or(from + Duration::days(6));
    if to < from {
        return Err(AppError::BadRequest("`to` is before `from`".into()));
    }
    let span = (to - from).num_days() + 1;
    if span > MAX_DAYS {
        return Err(AppError::BadRequest(format!(
            "pick at most {MAX_DAYS} days at a time"
        )));
    }
    let days: Vec<NaiveDate> = (0..span).map(|i| from + Duration::days(i)).collect();
    let start_ts = Utc.from_utc_datetime(&NaiveDateTime::new(from, NaiveTime::MIN));
    let end_ts = Utc.from_utc_datetime(&NaiveDateTime::new(to + Duration::days(1), NaiveTime::MIN));

    #[derive(sqlx::FromRow)]
    struct M {
        user_id: Uuid,
        handle: String,
        display_name: String,
        avatar_url: Option<String>,
        avatar_style: Option<String>,
        avatar_seed: Option<String>,
        role: String,
    }
    let members: Vec<M> = sqlx::query_as(
        r#"
        SELECT u.id AS user_id, u.handle, u.display_name, u.avatar_url, u.avatar_style,
               u.avatar_seed, pm.role
        FROM   project_members pm
        JOIN   users u ON u.id = pm.user_id
        WHERE  pm.project_id = $1 AND u.deleted_at IS NULL
        ORDER  BY lower(u.handle)
        "#,
    )
    .bind(ctx.id)
    .fetch_all(&state.db)
    .await?;

    let logs: Vec<(Uuid, DateTime<Utc>, i64)> = sqlx::query_as(
        r#"
        SELECT tl.user_id, tl.started_at, COALESCE(tl.duration_minutes, 0)::int8
        FROM   time_logs tl
        JOIN   tasks t ON t.id = tl.task_id
        WHERE  t.project_id = $1
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2 AND tl.started_at < $3
          AND  ($4::uuid IS NULL OR tl.user_id = $4)
        "#,
    )
    .bind(ctx.id)
    .bind(start_ts)
    .bind(end_ts)
    .bind(if team_scope { None } else { Some(user.id) })
    .fetch_all(&state.db)
    .await?;
    let by_user = bucket_by_day(from, days.len(), &logs);

    // KPI sprint: the one asked for, else the running one, else the latest
    // completed — so a team between sprints still sees how the last went.
    #[derive(sqlx::FromRow)]
    struct S {
        id: Uuid,
        name: String,
        state: String,
        snap: bool,
    }
    let sprint: Option<S> = sqlx::query_as(
        r#"
        SELECT s.id, s.name, s.state,
               (s.state = 'completed' AND EXISTS (
                   SELECT 1 FROM sprint_task_snapshots x WHERE x.sprint_id = s.id)) AS snap
        FROM   sprints s
        WHERE  s.project_id = $1 AND s.deleted_at IS NULL
          AND  ($2::uuid IS NULL OR s.id = $2)
          AND  ($2::uuid IS NOT NULL OR s.state IN ('active', 'completed'))
        ORDER  BY (s.state = 'active') DESC, s.completed_at DESC NULLS LAST
        LIMIT  1
        "#,
    )
    .bind(ctx.id)
    .bind(q.sprint_id)
    .fetch_optional(&state.db)
    .await?;
    if q.sprint_id.is_some() && sprint.is_none() {
        return Err(AppError::NotFound);
    }

    let mut kpis: HashMap<Uuid, MemberKpi> = HashMap::new();
    if let Some(s) = &sprint {
        #[derive(sqlx::FromRow)]
        struct K {
            assignee_id: Uuid,
            assigned: i64,
            completed: i64,
            completed_with_due: i64,
            on_time: i64,
            estimated: i64,
        }
        // A completed sprint answers from its snapshot (who had what, and
        // whether it was done, *at completion*); due dates, completion times
        // and estimates come from the live task.
        let sql = if s.snap {
            r#"
            SELECT x.assignee_id,
                   count(*)::int8 AS assigned,
                   count(*) FILTER (WHERE x.status = 'done')::int8 AS completed,
                   count(*) FILTER (WHERE x.status = 'done' AND t.due_date IS NOT NULL)::int8
                       AS completed_with_due,
                   count(*) FILTER (WHERE x.status = 'done' AND t.due_date IS NOT NULL
                                    AND t.completed_at IS NOT NULL
                                    AND t.completed_at::date <= t.due_date)::int8 AS on_time,
                   count(*) FILTER (WHERE t.estimate_minutes IS NOT NULL
                                    OR x.story_points IS NOT NULL)::int8 AS estimated
            FROM   sprint_task_snapshots x
            LEFT JOIN tasks t ON t.id = x.task_id
            WHERE  x.sprint_id = $1 AND x.assignee_id IS NOT NULL
            GROUP  BY x.assignee_id
            "#
        } else {
            r#"
            SELECT t.assignee_id,
                   count(*)::int8 AS assigned,
                   count(*) FILTER (WHERE t.status = 'done')::int8 AS completed,
                   count(*) FILTER (WHERE t.status = 'done' AND t.due_date IS NOT NULL)::int8
                       AS completed_with_due,
                   count(*) FILTER (WHERE t.status = 'done' AND t.due_date IS NOT NULL
                                    AND t.completed_at IS NOT NULL
                                    AND t.completed_at::date <= t.due_date)::int8 AS on_time,
                   count(*) FILTER (WHERE t.estimate_minutes IS NOT NULL
                                    OR t.story_points IS NOT NULL)::int8 AS estimated
            FROM   tasks t
            WHERE  t.sprint_id = $1 AND t.deleted_at IS NULL
              AND  t.parent_task_id IS NULL AND t.assignee_id IS NOT NULL
            GROUP  BY t.assignee_id
            "#
        };
        let rows: Vec<K> = sqlx::query_as(sql).bind(s.id).fetch_all(&state.db).await?;
        for r in rows {
            kpis.insert(
                r.assignee_id,
                MemberKpi {
                    assigned: r.assigned,
                    completed: r.completed,
                    completed_with_due: r.completed_with_due,
                    on_time: r.on_time,
                    estimated: r.estimated,
                },
            );
        }
    }

    let mut totals_by_day = vec![0i64; days.len()];
    let rows: Vec<MemberRow> = members
        .into_iter()
        .map(|m| {
            let visible = team_scope || m.user_id == user.id;
            let minutes = visible.then(|| {
                by_user
                    .get(&m.user_id)
                    .cloned()
                    .unwrap_or_else(|| vec![0; days.len()])
            });
            if let Some(ms) = &minutes {
                for (i, v) in ms.iter().enumerate() {
                    totals_by_day[i] += v;
                }
            }
            MemberRow {
                total_minutes: minutes.as_ref().map(|ms| ms.iter().sum()),
                minutes_by_day: minutes,
                kpi: kpis.remove(&m.user_id).unwrap_or_default(),
                user_id: m.user_id,
                handle: m.handle,
                display_name: m.display_name,
                avatar_url: m.avatar_url,
                avatar_style: m.avatar_style,
                avatar_seed: m.avatar_seed,
                role: m.role,
            }
        })
        .collect();

    Ok(Json(serde_json::json!({
        "from": from,
        "to": to,
        "days": days,
        "scope": if team_scope { "team" } else { "self" },
        "sprint": sprint.as_ref().map(|s| serde_json::json!({
            "id": s.id, "name": s.name, "state": s.state, "snapshot": s.snap,
        })),
        "members": rows,
        "totals_by_day": totals_by_day,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_land_in_their_utc_day() {
        let from = NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let at = |d: u32, h: u32| Utc.with_ymd_and_hms(2026, 9, d, h, 0, 0).unwrap();
        let logs = vec![
            (a, at(28, 9), 60),
            (a, at(28, 23), 30),
            (a, at(30, 10), 45),
            (b, at(29, 0), 120),
            // Outside the 3-day window, and a zero: both ignored.
            (b, Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(), 999),
            (b, at(27, 12), 999),
            (b, at(29, 1), 0),
        ];
        let m = bucket_by_day(from, 3, &logs);
        assert_eq!(m[&a], vec![90, 0, 45]);
        assert_eq!(m[&b], vec![0, 120, 0]);
    }

    #[test]
    fn weeks_start_on_monday() {
        let thu = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        assert_eq!(
            monday_of(thu),
            NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
        );
        let mon = NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        assert_eq!(monday_of(mon), mon);
        let sun = NaiveDate::from_ymd_opt(2026, 10, 4).unwrap();
        assert_eq!(monday_of(sun), mon);
    }
}
