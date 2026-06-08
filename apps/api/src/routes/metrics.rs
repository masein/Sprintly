//! Flow metrics for a project: lead time, weekly throughput, current WIP.
//!
//!   GET /projects/:key/metrics?weeks=N   (default 8, clamped 1..=52)

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{
    domain::{
        metrics,
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/projects/:key/metrics", get(get_metrics))
}

#[derive(Debug, Deserialize)]
struct MetricsQuery {
    weeks: Option<i64>,
}

async fn get_metrics(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Query(q): Query<MetricsQuery>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let m = metrics::compute(&state.db, ctx.id, q.weeks.unwrap_or(8)).await?;
    Ok(Json(m))
}
