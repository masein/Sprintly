//! Per-client billing endpoints (F14). All admin-only.
//!
//!   GET/POST   /clients                  — list / create
//!   PATCH/DEL  /clients/:id              — edit / soft-delete
//!   PUT        /projects/:key/client     — assign/clear a project's client
//!   GET/POST   /invoices                 — list / generate
//!   GET/DEL    /invoices/:id             — fetch (+lines) / delete draft
//!   GET        /invoices/:id/pdf|csv     — exports (reuse PdfBuilder, csv)
//!   POST       /invoices/:id/mark-sent|mark-paid

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{invoicing, permissions::Role as GlobalRole},
    infra::{pdf::PdfBuilder, AppState},
    middleware::CurrentUser,
    AppError, AppResult,
};

fn require_admin(user: &CurrentUser) -> AppResult<()> {
    if user.role == GlobalRole::Admin {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/clients", get(list_clients).post(create_client))
        .route(
            "/clients/:id",
            axum::routing::patch(update_client).delete(delete_client),
        )
        .route("/projects/:key/client", put(set_project_client))
        .route("/invoices", get(list_invoices).post(create_invoice))
        .route("/invoices/:id", get(get_invoice).delete(delete_invoice))
        .route("/invoices/:id/pdf", get(invoice_pdf))
        .route("/invoices/:id/csv", get(invoice_csv))
        .route("/invoices/:id/mark-sent", post(mark_sent))
        .route("/invoices/:id/mark-paid", post(mark_paid))
}

// ─── clients ─────────────────────────────────────────────────────────────────

async fn list_clients(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    Ok(Json(invoicing::list_clients(&state.db).await?))
}

#[derive(Debug, Deserialize)]
struct CreateClientReq {
    name: String,
    email: Option<String>,
    address: Option<String>,
    currency: Option<String>,
    notes: Option<String>,
}

async fn create_client(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateClientReq>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    if req.name.trim().is_empty() {
        return Err(AppError::Validation("client name is required".into()));
    }
    let c = invoicing::create_client(
        &state.db,
        user.id,
        invoicing::NewClient {
            name: req.name,
            email: req.email,
            address: req.address,
            currency: req.currency,
            notes: req.notes,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(c)))
}

#[derive(Debug, Deserialize)]
struct UpdateClientReq {
    name: Option<String>,
    email: Option<String>,
    address: Option<String>,
    currency: Option<String>,
    notes: Option<String>,
}

async fn update_client(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateClientReq>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let c = invoicing::update_client(
        &state.db,
        id,
        req.name,
        req.email,
        req.address,
        req.currency,
        req.notes,
    )
    .await?;
    Ok(Json(c))
}

async fn delete_client(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    invoicing::delete_client(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct SetClientReq {
    client_id: Option<Uuid>,
}

async fn set_project_client(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(req): Json<SetClientReq>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    if !invoicing::set_project_client(&state.db, &key, req.client_id).await? {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── invoices ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ListInvoicesQuery {
    client_id: Option<Uuid>,
}

async fn list_invoices(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListInvoicesQuery>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let items = invoicing::list_invoices(&state.db, q.client_id).await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
struct CreateInvoiceReq {
    client_id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
}

async fn create_invoice(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateInvoiceReq>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let id = invoicing::generate(
        &state.db,
        req.client_id,
        req.period_start,
        req.period_end,
        user.id,
    )
    .await?;
    let inv = invoicing::fetch(&state.db, id).await?;
    Ok((StatusCode::CREATED, Json(inv)))
}

async fn get_invoice(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    Ok(Json(invoicing::fetch(&state.db, id).await?))
}

async fn delete_invoice(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    invoicing::delete_draft(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_sent(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    invoicing::mark_sent(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_paid(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    invoicing::mark_paid(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── exports ─────────────────────────────────────────────────────────────────

fn money(cents: i64) -> String {
    // Display only; the stored value is integer cents.
    format!("{}.{:02}", cents / 100, (cents % 100).abs())
}

fn hours(minutes: i64) -> String {
    format!("{:.2}", minutes as f64 / 60.0)
}

async fn invoice_pdf(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let inv = invoicing::fetch(&state.db, id).await?;

    let mut pdf = PdfBuilder::new();
    pdf.text_top(50.0, 60.0, 20.0, "Sprintly — Invoice");
    pdf.text_top(
        50.0,
        88.0,
        12.0,
        &format!("Invoice: {}", inv.invoice.number),
    );
    pdf.text_top(50.0, 106.0, 12.0, &format!("Bill to: {}", inv.client_name));
    pdf.text_top(
        50.0,
        124.0,
        11.0,
        &format!(
            "Period: {} → {}",
            inv.invoice.period_start, inv.invoice.period_end
        ),
    );
    pdf.text_top(
        50.0,
        140.0,
        11.0,
        &format!("Status: {}", inv.invoice.status),
    );

    // Table header.
    pdf.text_top(50.0, 180.0, 11.0, "Description");
    pdf.text_top(330.0, 180.0, 11.0, "Hours");
    pdf.text_top(400.0, 180.0, 11.0, "Rate");
    pdf.text_top(490.0, 180.0, 11.0, "Amount");

    let mut y = 200.0;
    for line in &inv.lines {
        pdf.text_top(50.0, y, 10.0, &line.description);
        pdf.text_top(330.0, y, 10.0, &hours(line.minutes));
        pdf.text_top(400.0, y, 10.0, &money(line.rate_cents));
        pdf.text_top(490.0, y, 10.0, &money(line.amount_cents));
        y += 16.0;
    }
    y += 14.0;
    pdf.text_top(
        400.0,
        y,
        13.0,
        &format!(
            "Total: {} {}",
            inv.invoice.currency,
            money(inv.invoice.total_cents)
        ),
    );
    pdf.text_top(
        50.0,
        740.0,
        9.0,
        &format!("Generated by Sprintly · {}", Utc::now().to_rfc3339()),
    );

    let bytes = pdf.finish();
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}.pdf\"",
            inv.invoice.number
        ))
        .unwrap(),
    );
    Ok((StatusCode::OK, h, bytes).into_response())
}

async fn invoice_csv(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    require_admin(&user)?;
    let inv = invoicing::fetch(&state.db, id).await?;

    let mut csv = String::from("description,minutes,hours,rate_cents,amount_cents\n");
    for line in &inv.lines {
        csv.push_str(&format!(
            "{},{},{},{},{}\n",
            csv_escape(&line.description),
            line.minutes,
            hours(line.minutes),
            line.rate_cents,
            line.amount_cents,
        ));
    }
    csv.push_str(&format!("TOTAL,,,,{}\n", inv.invoice.total_cents));

    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}.csv\"",
            inv.invoice.number
        ))
        .unwrap(),
    );
    Ok((StatusCode::OK, h, csv).into_response())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
