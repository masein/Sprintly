//! Import / export endpoints (F16).
//!
//!   POST /projects/:key/import   — dry-run or apply a Trello/CSV import (lead)
//!   GET  /projects/:key/export   — JSON bundle or CSV of the project (viewer)

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        import_export::{self, ImportFormat},
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/import", post(import))
        .route("/projects/:key/export", get(export))
}

#[derive(Debug, Deserialize)]
struct ImportReq {
    /// "trello" | "csv" | "auto" (default auto).
    #[serde(default)]
    format: Option<String>,
    /// Raw file contents.
    content: String,
    /// When true, resolve + report but don't persist.
    #[serde(default)]
    dry_run: bool,
}

async fn import(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<ImportReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let requested = ImportFormat::parse(req.format.as_deref().unwrap_or("auto"));
    let format = import_export::resolve_format(&req.content, requested);

    let board_id = default_board(&state.db, ctx.id).await?;
    let report = if format == ImportFormat::Jira {
        let plan = crate::domain::jira::parse_jira_csv(&req.content)?;
        import_export::apply_jira_import(&state.db, ctx.id, board_id, &plan, req.dry_run).await?
    } else {
        let plan = import_export::parse(&req.content, format)?;
        import_export::apply_import(&state.db, ctx.id, board_id, &plan, req.dry_run).await?
    };
    Ok(Json(report))
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    #[serde(default)]
    format: Option<String>,
}

async fn export(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Query(q): Query<ExportQuery>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }

    let bundle = import_export::export_bundle(&state.db, ctx.id).await?;

    if q.format.as_deref() == Some("csv") {
        let csv = import_export::export_csv(&bundle);
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/csv; charset=utf-8"),
        );
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{}-export.csv\"", ctx.key))
                .unwrap(),
        );
        return Ok((StatusCode::OK, h, csv).into_response());
    }

    let body = serde_json::to_string_pretty(&bundle)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("serialize export: {e}")))?;
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}-export.json\"", ctx.key))
            .unwrap(),
    );
    Ok((StatusCode::OK, h, body).into_response())
}

/// The project's default board (or the oldest if none is flagged default).
async fn default_board(db: &sqlx::PgPool, project_id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar(
        r#"SELECT id FROM boards
            WHERE project_id = $1 AND deleted_at IS NULL
            ORDER BY is_default DESC, created_at
            LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::BadRequest("project has no board to import into".into()))
}
