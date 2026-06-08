//! Flow-metrics computation: lead time, weekly throughput, current WIP.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::AppResult;

#[derive(Debug, Serialize, FromRow)]
pub struct LeadTime {
    pub count: i64,
    pub avg_hours: f64,
    pub p50_hours: f64,
    pub p90_hours: f64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ThroughputPoint {
    pub week_start: NaiveDate,
    pub count: i64,
}

#[derive(Debug, Serialize, Default)]
pub struct Wip {
    pub todo: i64,
    pub in_progress: i64,
    pub review: i64,
}

#[derive(Debug, Serialize)]
pub struct Metrics {
    pub weeks: i64,
    pub lead_time: LeadTime,
    pub throughput: Vec<ThroughputPoint>,
    pub wip: Wip,
}

/// Compute flow metrics for a project over the trailing `weeks` window.
pub async fn compute(db: &PgPool, project_id: Uuid, weeks: i64) -> AppResult<Metrics> {
    let weeks = weeks.clamp(1, 52);
    let since: DateTime<Utc> = Utc::now() - Duration::weeks(weeks);

    let lead_time: LeadTime = sqlx::query_as(
        r#"
        SELECT count(*)::int8 AS count,
               COALESCE(avg(EXTRACT(EPOCH FROM (completed_at - created_at)) / 3600.0), 0)::float8 AS avg_hours,
               COALESCE(percentile_cont(0.5) WITHIN GROUP (
                   ORDER BY EXTRACT(EPOCH FROM (completed_at - created_at)) / 3600.0), 0)::float8 AS p50_hours,
               COALESCE(percentile_cont(0.9) WITHIN GROUP (
                   ORDER BY EXTRACT(EPOCH FROM (completed_at - created_at)) / 3600.0), 0)::float8 AS p90_hours
        FROM   tasks
        WHERE  project_id = $1 AND deleted_at IS NULL
          AND  completed_at IS NOT NULL AND completed_at >= $2
        "#,
    )
    .bind(project_id)
    .bind(since)
    .fetch_one(db)
    .await?;

    let throughput: Vec<ThroughputPoint> = sqlx::query_as(
        r#"
        SELECT gs::date AS week_start, COALESCE(t.cnt, 0)::int8 AS count
        FROM   generate_series(
                   date_trunc('week', $2::timestamptz),
                   date_trunc('week', now()),
                   interval '1 week') gs
        LEFT JOIN (
            SELECT date_trunc('week', completed_at) AS wk, count(*) AS cnt
            FROM   tasks
            WHERE  project_id = $1 AND deleted_at IS NULL
              AND  completed_at IS NOT NULL AND completed_at >= $2
            GROUP  BY 1
        ) t ON t.wk = gs
        ORDER  BY gs
        "#,
    )
    .bind(project_id)
    .bind(since)
    .fetch_all(db)
    .await?;

    let wip_rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT status, count(*)::int8
           FROM tasks
           WHERE project_id = $1 AND deleted_at IS NULL
             AND status IN ('todo', 'in_progress', 'review')
           GROUP BY status"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    let mut wip = Wip::default();
    for (status, c) in wip_rows {
        match status.as_str() {
            "todo" => wip.todo = c,
            "in_progress" => wip.in_progress = c,
            "review" => wip.review = c,
            _ => {}
        }
    }

    Ok(Metrics {
        weeks,
        lead_time,
        throughput,
        wip,
    })
}
