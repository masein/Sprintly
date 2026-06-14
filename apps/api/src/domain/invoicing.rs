//! Per-client billing (F14). Clients own projects; an invoice rolls up billable
//! time on a client's projects over a period into line items, each priced at the
//! contributor's configured hourly rate. All money is integer cents and reuses
//! [`crate::domain::timesheets::pay_cents`] (minutes × rate ÷ 60, floored), so
//! invoice totals match the underlying time × rate exactly.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::timesheets::pay_cents, AppError, AppResult};

// ─── clients ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Client {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub address: Option<String>,
    pub currency: String,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

pub struct NewClient {
    pub name: String,
    pub email: Option<String>,
    pub address: Option<String>,
    pub currency: Option<String>,
    pub notes: Option<String>,
}

pub async fn create_client(db: &PgPool, by: Uuid, n: NewClient) -> AppResult<Client> {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO clients (id, name, email, address, currency, notes, created_by)
           VALUES ($1, $2, $3, $4, COALESCE($5, 'USD'), COALESCE($6, ''), $7)"#,
    )
    .bind(id)
    .bind(n.name.trim())
    .bind(n.email.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(n.address)
    .bind(n.currency.map(|c| c.trim().to_uppercase()))
    .bind(n.notes)
    .bind(by)
    .execute(db)
    .await?;
    get_client(db, id).await
}

pub async fn list_clients(db: &PgPool) -> AppResult<Vec<Client>> {
    Ok(sqlx::query_as::<_, Client>(
        r#"SELECT id, name, email, address, currency, notes, created_at
             FROM clients WHERE deleted_at IS NULL ORDER BY name"#,
    )
    .fetch_all(db)
    .await?)
}

pub async fn get_client(db: &PgPool, id: Uuid) -> AppResult<Client> {
    sqlx::query_as::<_, Client>(
        r#"SELECT id, name, email, address, currency, notes, created_at
             FROM clients WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

/// Update the fields that were supplied (set-only; an omitted field is left
/// untouched via COALESCE).
pub async fn update_client(
    db: &PgPool,
    id: Uuid,
    name: Option<String>,
    email: Option<String>,
    address: Option<String>,
    currency: Option<String>,
    notes: Option<String>,
) -> AppResult<Client> {
    sqlx::query(
        r#"UPDATE clients SET
             name     = COALESCE($2, name),
             email    = COALESCE($3, email),
             address  = COALESCE($4, address),
             currency = COALESCE($5, currency),
             notes    = COALESCE($6, notes),
             updated_at = now()
           WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .bind(name)
    .bind(email)
    .bind(address)
    .bind(currency.map(|c| c.trim().to_uppercase()))
    .bind(notes)
    .execute(db)
    .await?;
    get_client(db, id).await
}

pub async fn delete_client(db: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE clients SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Assign (or clear, with `None`) a project's client. Returns false if the
/// project key is unknown.
pub async fn set_project_client(
    db: &PgPool,
    project_key: &str,
    client_id: Option<Uuid>,
) -> AppResult<bool> {
    let res = sqlx::query(
        r#"UPDATE projects SET client_id = $2, updated_at = now()
            WHERE key = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_key)
    .bind(client_id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() == 1)
}

// ─── invoices ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Invoice {
    pub id: Uuid,
    pub client_id: Uuid,
    pub number: String,
    pub status: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub currency: String,
    pub subtotal_cents: i64,
    pub total_cents: i64,
    pub notes: String,
    pub issued_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct InvoiceLine {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub description: String,
    pub minutes: i64,
    pub rate_cents: i64,
    pub amount_cents: i64,
    pub sort: i32,
}

#[derive(Debug, Serialize)]
pub struct InvoiceWithLines {
    #[serde(flatten)]
    pub invoice: Invoice,
    pub client_name: String,
    pub lines: Vec<InvoiceLine>,
}

fn day_bounds(start: NaiveDate, end: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let s = Utc.from_utc_datetime(&NaiveDateTime::new(start, NaiveTime::MIN));
    // period_end is inclusive → up to the start of the following day.
    let e = Utc.from_utc_datetime(&NaiveDateTime::new(
        end + chrono::Duration::days(1),
        NaiveTime::MIN,
    ));
    (s, e)
}

/// Generate a draft invoice for a client over `[start, end]` (inclusive),
/// one line per (project, contributor) with billable time. Rejected if there's
/// no billable time in the window. Returns the new invoice id.
pub async fn generate(
    db: &PgPool,
    client_id: Uuid,
    start: NaiveDate,
    end: NaiveDate,
    by: Uuid,
) -> AppResult<Uuid> {
    if end < start {
        return Err(AppError::BadRequest("period end is before start".into()));
    }
    let client = get_client(db, client_id).await?;
    let (start_ts, end_ts) = day_bounds(start, end);

    // (project, user) billable-minute rollup with each user's configured rate.
    let rows = sqlx::query_as::<_, (Uuid, String, String, i64, i64)>(
        r#"
        SELECT t.project_id,
               p.key                              AS project_key,
               u.handle,
               COALESCE(u.hourly_rate_cents, 0)   AS rate_cents,
               COALESCE(SUM(tl.duration_minutes), 0)::bigint AS minutes
        FROM   time_logs tl
        JOIN   tasks t    ON t.id = tl.task_id
        JOIN   projects p ON p.id = t.project_id
        JOIN   users u    ON u.id = tl.user_id
        WHERE  p.client_id = $1
          AND  tl.billable = true
          AND  tl.deleted_at IS NULL
          AND  tl.ended_at IS NOT NULL
          AND  tl.started_at >= $2 AND tl.started_at < $3
        GROUP  BY t.project_id, p.key, u.handle, u.hourly_rate_cents
        HAVING COALESCE(SUM(tl.duration_minutes), 0) > 0
        ORDER  BY p.key, u.handle
        "#,
    )
    .bind(client_id)
    .bind(start_ts)
    .bind(end_ts)
    .fetch_all(db)
    .await?;

    let lines: Vec<(Uuid, String, i64, i64, i64)> = rows
        .into_iter()
        .map(|(project_id, key, handle, rate, minutes)| {
            let amount = pay_cents(minutes, Some(rate));
            let desc = format!("{key} — @{handle}");
            (project_id, desc, minutes, rate, amount)
        })
        .filter(|(_, _, _, _, amount)| *amount > 0)
        .collect();

    if lines.is_empty() {
        return Err(AppError::BadRequest(
            "no billable time for this client in the period".into(),
        ));
    }
    let subtotal: i64 = lines.iter().map(|l| l.4).sum();

    let mut tx = db.begin().await?;
    // Human-friendly sequential number, scoped to the period's year.
    let year = end.format("%Y").to_string();
    let seq: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM invoices WHERE to_char(period_end, 'YYYY') = $1"#,
    )
    .bind(&year)
    .fetch_one(&mut *tx)
    .await?;
    let number = format!("INV-{year}-{:04}", seq + 1);

    let invoice_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO invoices
             (id, client_id, number, status, period_start, period_end, currency,
              subtotal_cents, total_cents, created_by)
           VALUES ($1, $2, $3, 'draft', $4, $5, $6, $7, $7, $8)"#,
    )
    .bind(invoice_id)
    .bind(client_id)
    .bind(&number)
    .bind(start)
    .bind(end)
    .bind(&client.currency)
    .bind(subtotal)
    .bind(by)
    .execute(&mut *tx)
    .await?;

    for (i, (project_id, desc, minutes, rate, amount)) in lines.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO invoice_lines
                 (id, invoice_id, project_id, description, minutes, rate_cents, amount_cents, sort)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .bind(project_id)
        .bind(desc)
        .bind(minutes)
        .bind(rate)
        .bind(amount)
        .bind(i as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(invoice_id)
}

pub async fn fetch(db: &PgPool, id: Uuid) -> AppResult<InvoiceWithLines> {
    let invoice = sqlx::query_as::<_, Invoice>(
        r#"SELECT id, client_id, number, status, period_start, period_end, currency,
                  subtotal_cents, total_cents, notes, issued_at, sent_at, paid_at, created_at
             FROM invoices WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    let client_name: String = sqlx::query_scalar("SELECT name FROM clients WHERE id = $1")
        .bind(invoice.client_id)
        .fetch_one(db)
        .await?;

    let lines = sqlx::query_as::<_, InvoiceLine>(
        r#"SELECT id, project_id, description, minutes, rate_cents, amount_cents, sort
             FROM invoice_lines WHERE invoice_id = $1 ORDER BY sort"#,
    )
    .bind(id)
    .fetch_all(db)
    .await?;

    Ok(InvoiceWithLines {
        invoice,
        client_name,
        lines,
    })
}

pub async fn list_invoices(db: &PgPool, client_id: Option<Uuid>) -> AppResult<Vec<Invoice>> {
    Ok(sqlx::query_as::<_, Invoice>(
        r#"SELECT id, client_id, number, status, period_start, period_end, currency,
                  subtotal_cents, total_cents, notes, issued_at, sent_at, paid_at, created_at
             FROM invoices
            WHERE ($1::uuid IS NULL OR client_id = $1)
            ORDER BY created_at DESC
            LIMIT 200"#,
    )
    .bind(client_id)
    .fetch_all(db)
    .await?)
}

/// draft → sent.
pub async fn mark_sent(db: &PgPool, id: Uuid) -> AppResult<()> {
    let res = sqlx::query(
        "UPDATE invoices SET status = 'sent', sent_at = now() WHERE id = $1 AND status = 'draft'",
    )
    .bind(id)
    .execute(db)
    .await?;
    if res.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AppError::Conflict(
            "only a draft invoice can be sent".into(),
        ))
    }
}

/// → paid (from draft or sent).
pub async fn mark_paid(db: &PgPool, id: Uuid) -> AppResult<()> {
    let res = sqlx::query(
        "UPDATE invoices SET status = 'paid', paid_at = now() WHERE id = $1 AND status <> 'paid'",
    )
    .bind(id)
    .execute(db)
    .await?;
    if res.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AppError::Conflict("invoice is already paid".into()))
    }
}

/// Delete a draft invoice (its lines cascade). Sent/paid invoices are kept as a
/// financial record.
pub async fn delete_draft(db: &PgPool, id: Uuid) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM invoices WHERE id = $1 AND status = 'draft'")
        .bind(id)
        .execute(db)
        .await?;
    if res.rows_affected() == 1 {
        Ok(())
    } else {
        Err(AppError::Conflict(
            "only a draft invoice can be deleted".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_bounds_make_end_date_inclusive() {
        let start = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let (s, e) = day_bounds(start, end);
        assert_eq!(s.to_rfc3339(), "2026-05-01T00:00:00+00:00");
        // Exclusive upper bound is the start of June 1 → all of May 31 counts.
        assert_eq!(e.to_rfc3339(), "2026-06-01T00:00:00+00:00");
    }

    #[test]
    fn line_amount_is_minutes_times_rate_floored() {
        // Even division: 60 min @ $60/hr = $60.00.
        assert_eq!(pay_cents(60, Some(6000)), 6000);
        // Rounding down: 25 min @ $100/hr = 4166.67¢ → 4166¢.
        assert_eq!(pay_cents(25, Some(10_000)), 4166);
        // Zero rate or no time bills nothing (line is dropped).
        assert_eq!(pay_cents(60, Some(0)), 0);
        assert_eq!(pay_cents(0, Some(6000)), 0);
    }
}
