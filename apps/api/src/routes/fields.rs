//! Per-project custom fields + per-task values.
//!
//!   GET    /projects/:key/fields                 — list definitions
//!   POST   /projects/:key/fields                 — { name, type, options? }
//!   PATCH  /fields/:id                           — { name?, options? } (type immutable)
//!   DELETE /fields/:id                           — also drops its task values
//!   GET    /tasks/:task_key/fields               — all fields + this task's values
//!   PUT    /tasks/:task_key/fields/:field_id     — { value }
//!   DELETE /tasks/:task_key/fields/:field_id     — clear (idempotent)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        fields,
        permissions::{can, Action},
        projects as project_ctx, tasks as task_domain,
    },
    infra::{events::Event, AppState},
    middleware::CurrentUser,
    AppError, AppResult,
};

use super::tasks::resolve_project_from_task_key;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:key/fields", get(list_defs).post(create_def))
        .route(
            "/fields/:id",
            axum::routing::patch(update_def).delete(delete_def),
        )
        .route("/tasks/:task_key/fields", get(list_values))
        .route(
            "/tasks/:task_key/fields/:field_id",
            axum::routing::put(set_value).delete(clear_value),
        )
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    name: String,
    r#type: String,
    options: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    name: Option<String>,
    options: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SetValueReq {
    value: String,
}

fn check_name(name: &str) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > 40 {
        return Err(AppError::BadRequest("field name must be 1–40 chars".into()));
    }
    // '=' splits name from value in the filter DSL; ':' and '+' frame tokens.
    if n.contains(['=', ':', '+']) {
        return Err(AppError::BadRequest(
            "field name must not contain '=', ':' or '+'".into(),
        ));
    }
    Ok(())
}

/// Trim, drop empties, reject silly inputs. Select needs at least one option;
/// the other types carry none.
fn check_options(field_type: &str, options: Option<Vec<String>>) -> AppResult<Vec<String>> {
    let opts: Vec<String> = options
        .unwrap_or_default()
        .into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    match field_type {
        "select" => {
            if opts.is_empty() {
                return Err(AppError::BadRequest(
                    "a select field needs at least one option".into(),
                ));
            }
            if opts.len() > fields::MAX_OPTIONS {
                return Err(AppError::BadRequest(format!(
                    "too many options (max {})",
                    fields::MAX_OPTIONS
                )));
            }
            if opts.iter().any(|o| o.len() > 100) {
                return Err(AppError::BadRequest(
                    "option too long (max 100 chars)".into(),
                ));
            }
            for (i, o) in opts.iter().enumerate() {
                if opts[..i].iter().any(|p| p.eq_ignore_ascii_case(o)) {
                    return Err(AppError::BadRequest(format!("duplicate option: {o}")));
                }
            }
            Ok(opts)
        }
        _ => {
            if opts.is_empty() {
                Ok(opts)
            } else {
                Err(AppError::BadRequest(
                    "options only make sense on a select field".into(),
                ))
            }
        }
    }
}

// ─── definitions ────────────────────────────────────────────────────────────

async fn list_defs(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::ViewProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    Ok(Json(fields::list(&state.db, ctx.id).await?))
}

async fn create_def(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_key): Path<String>,
    Json(req): Json<CreateReq>,
) -> AppResult<impl IntoResponse> {
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    check_name(&req.name)?;
    if !fields::valid_field_type(&req.r#type) {
        return Err(AppError::BadRequest(format!(
            "type must be one of: {}",
            fields::FIELD_TYPES.join(", ")
        )));
    }
    let options = check_options(&req.r#type, req.options)?;
    let field = fields::create(&state.db, ctx.id, req.name.trim(), &req.r#type, &options).await?;
    Ok((StatusCode::CREATED, Json(field)))
}

async fn update_def(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateReq>,
) -> AppResult<impl IntoResponse> {
    let field = fields::get(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, field.project_id, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    if let Some(name) = &req.name {
        check_name(name)?;
    }
    let options = match req.options {
        Some(opts) => Some(check_options(&field.r#type, Some(opts))?),
        None => None,
    };
    let field = fields::update(
        &state.db,
        id,
        field.project_id,
        req.name.as_deref().map(str::trim),
        options.as_deref(),
    )
    .await?;
    Ok(Json(field))
}

async fn delete_def(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let pid = fields::project_of(&state.db, id).await?;
    let ctx = project_ctx::load_by_id(&state.db, pid, user.id).await?;
    if !can(&user.as_actor(), Action::EditProject, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    fields::delete(&state.db, id, pid).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── task values ────────────────────────────────────────────────────────────

/// Resolve the task + check the actor can act on it. Returns
/// (task_id, project_id, project_key).
async fn resolve_task(
    state: &AppState,
    user: &CurrentUser,
    task_key: &str,
    action: Action,
) -> AppResult<(Uuid, Uuid, String)> {
    let (project_id, project_key) = resolve_project_from_task_key(&state.db, task_key).await?;
    let ctx = project_ctx::load_by_key(&state.db, &project_key, user.id).await?;
    if !can(&user.as_actor(), action, ctx.as_resource()) {
        return Err(AppError::Forbidden);
    }
    let task_id: Uuid = sqlx::query_scalar(
        r#"SELECT id FROM tasks WHERE key = $1 AND project_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(task_key)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok((task_id, project_id, project_key))
}

async fn list_values(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(task_key): Path<String>,
) -> AppResult<impl IntoResponse> {
    let (task_id, project_id, _) =
        resolve_task(&state, &user, &task_key, Action::ViewBoard).await?;
    Ok(Json(
        fields::list_for_task(&state.db, project_id, task_id).await?,
    ))
}

async fn set_value(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((task_key, field_id)): Path<(String, Uuid)>,
    Json(req): Json<SetValueReq>,
) -> AppResult<impl IntoResponse> {
    let (task_id, project_id, _) =
        resolve_task(&state, &user, &task_key, Action::EditProject).await?;
    let field = fields::get(&state.db, field_id).await?;
    if field.project_id != project_id {
        return Err(AppError::NotFound);
    }
    let canonical = fields::canonical_value(&field.r#type, &field.options, &req.value)?;

    let mut tx = state.db.begin().await?;
    fields::set_value(&mut *tx, task_id, field_id, &canonical).await?;
    task_domain::log_activity(
        &mut tx,
        task_id,
        Some(user.id),
        "field_set",
        &serde_json::json!({ "field": field.name, "value": canonical }),
    )
    .await?;
    tx.commit().await?;

    crate::infra::events::publish(
        &state.redis,
        &Event::TaskUpdated {
            project_id,
            task_id,
            key: task_key,
        },
    )
    .await;
    Ok(Json(serde_json::json!({
        "field_id": field_id,
        "value": canonical,
    })))
}

async fn clear_value(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((task_key, field_id)): Path<(String, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let (task_id, project_id, _) =
        resolve_task(&state, &user, &task_key, Action::EditProject).await?;
    let field = fields::get(&state.db, field_id).await?;
    if field.project_id != project_id {
        return Err(AppError::NotFound);
    }

    let mut tx = state.db.begin().await?;
    fields::clear_value(&mut *tx, task_id, field_id).await?;
    task_domain::log_activity(
        &mut tx,
        task_id,
        Some(user.id),
        "field_cleared",
        &serde_json::json!({ "field": field.name }),
    )
    .await?;
    tx.commit().await?;

    crate::infra::events::publish(
        &state.redis,
        &Event::TaskUpdated {
            project_id,
            task_id,
            key: task_key,
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
