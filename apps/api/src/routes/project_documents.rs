//! Project documents — roadmaps, requirement briefs, contracts — kept with the
//! project, shown on its dashboard (QA report 6).
//!
//!   GET    /projects/:key/documents            — list (anyone on the project)
//!   POST   /projects/:key/documents            — start an upload (leads)
//!   POST   /project-documents/:id/complete     — finish it (the uploader)
//!   GET    /project-documents/:id/download     — 302 to a fresh signed URL
//!   DELETE /project-documents/:id              — remove (leads, or the uploader)
//!
//! The same two-phase presigned flow as task attachments: the browser PUTs the
//! bytes straight to storage, the API only signs. Downloads go through the
//! API so access is checked at click time and no expiring URL sits in a page.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        permissions::{can, Action},
        projects as project_ctx,
    },
    infra::AppState,
    middleware::CurrentUser,
    routes::task_detail::presigner_for,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/documents", get(list).post(create))
        .route("/project-documents/:id/complete", post(complete))
        .route("/project-documents/:id/download", get(download))
        .route("/project-documents/:id", delete(remove))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DocumentDto {
    pub id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub uploader_handle: Option<String>,
    pub created_at: DateTime<Utc>,
    /// The stable API link — set once the upload is complete.
    #[sqlx(default)]
    pub download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    filename: String,
    mime_type: String,
}

#[derive(Debug, Deserialize)]
struct CompleteReq {
    size_bytes: i64,
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let mut rows: Vec<DocumentDto> = sqlx::query_as(
        r#"
        SELECT d.id, d.filename, d.mime_type, d.size_bytes, d.status,
               u.handle AS uploader_handle, d.created_at
        FROM   project_documents d
        LEFT JOIN users u ON u.id = d.uploader_id
        WHERE  d.project_id = $1 AND d.deleted_at IS NULL
        ORDER  BY d.created_at DESC
        "#,
    )
    .bind(ctx.id)
    .fetch_all(&state.db)
    .await?;
    for r in &mut rows {
        if r.status == "ready" {
            r.download_url = Some(format!("/api/v1/project-documents/{}/download", r.id));
        }
    }
    Ok(Json(serde_json::json!({ "items": rows })))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(key): Path<String>,
    Json(req): Json<CreateReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let filename = req.filename.trim();
    if filename.is_empty() || filename.len() > 255 {
        return Err(AppError::BadRequest(
            "filename must be 1–255 characters".into(),
        ));
    }
    let mime = req.mime_type.trim();
    if mime.is_empty() || mime.len() > 200 {
        return Err(AppError::BadRequest(
            "mime_type must be 1–200 characters".into(),
        ));
    }
    let id = Uuid::now_v7();
    let storage_key = format!("projects/{}/{}", ctx.id, id);
    sqlx::query(
        r#"INSERT INTO project_documents
               (id, project_id, uploader_id, filename, mime_type, storage_key)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(id)
    .bind(ctx.id)
    .bind(user.id)
    .bind(filename)
    .bind(mime)
    .bind(&storage_key)
    .execute(&state.db)
    .await?;
    let upload_url = presigner_for(&state, &headers).put(&storage_key, mime, 600);
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "upload_url": upload_url,
            "storage_key": storage_key,
            "expires_in": 600,
        })),
    ))
}

/// The row behind a document id, with what the access checks need.
#[derive(sqlx::FromRow)]
struct Doc {
    project_id: Uuid,
    uploader_id: Option<Uuid>,
    status: String,
    storage_key: String,
    filename: String,
}

async fn load(state: &AppState, id: Uuid) -> AppResult<Doc> {
    sqlx::query_as(
        r#"SELECT project_id, uploader_id, status, storage_key, filename
           FROM project_documents WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn complete(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CompleteReq>,
) -> AppResult<impl IntoResponse> {
    let doc = load(&state, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, doc.project_id, user.id).await?;
    if doc.uploader_id != Some(user.id)
        && !can(&user.as_actor(), Action::EditProject, ctx.as_resource())
    {
        return Err(AppError::Forbidden);
    }
    if doc.status != "pending" {
        return Err(AppError::Conflict("that upload is already finished".into()));
    }
    if req.size_bytes < 0 {
        return Err(AppError::BadRequest("size_bytes can't be negative".into()));
    }
    sqlx::query("UPDATE project_documents SET status = 'ready', size_bytes = $2 WHERE id = $1")
        .bind(id)
        .bind(req.size_bytes)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn download(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let doc = load(&state, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, doc.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if doc.status != "ready" {
        return Err(AppError::Conflict("that upload never finished".into()));
    }
    let url = presigner_for(&state, &headers).get(&doc.storage_key, Some(&doc.filename), 120);
    Ok((
        StatusCode::FOUND,
        [
            (header::LOCATION, url),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
    ))
}

async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let doc = load(&state, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, doc.project_id, user.id).await?;
    if doc.uploader_id != Some(user.id)
        && !can(&user.as_actor(), Action::EditProject, ctx.as_resource())
    {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE project_documents SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
