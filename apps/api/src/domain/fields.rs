//! Per-project custom field definitions + per-task values.
//!
//! A field is project-scoped schema (`name` + `type` + select `options`);
//! values live in `task_field_values`, one per (task, field), stored as
//! canonical text. `canonical_value` is the single place that parses and
//! normalises a raw value for a field type — both writes and filters go
//! through it so "3.50" and "3.5" agree.

use std::collections::HashSet;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{AppError, AppResult};

pub const FIELD_TYPES: [&str; 4] = ["text", "number", "select", "date"];
pub const MAX_TEXT_LEN: usize = 500;
pub const MAX_OPTIONS: usize = 50;

#[derive(Debug, Serialize, FromRow)]
pub struct CustomField {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub r#type: String,
    pub options: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// One row of the per-task value listing: every project field, with the
/// task's value where set.
#[derive(Debug, Serialize, FromRow)]
pub struct TaskFieldValue {
    pub field_id: Uuid,
    pub name: String,
    pub r#type: String,
    pub options: Vec<String>,
    pub value: Option<String>,
}

pub fn valid_field_type(s: &str) -> bool {
    FIELD_TYPES.contains(&s)
}

/// Parse + normalise a raw value for a field. Returns the canonical text we
/// store and filter on, or a BadRequest naming what's wrong.
pub fn canonical_value(field_type: &str, options: &[String], raw: &str) -> AppResult<String> {
    let v = raw.trim();
    if v.is_empty() {
        return Err(AppError::BadRequest("value must not be empty".into()));
    }
    match field_type {
        "text" => {
            if v.len() > MAX_TEXT_LEN {
                return Err(AppError::BadRequest(format!(
                    "text value too long (max {MAX_TEXT_LEN} chars)"
                )));
            }
            Ok(v.to_string())
        }
        "number" => {
            let n: f64 = v
                .parse()
                .map_err(|_| AppError::BadRequest("not a number".into()))?;
            if !n.is_finite() {
                return Err(AppError::BadRequest("not a finite number".into()));
            }
            Ok(n.to_string())
        }
        "select" => options
            .iter()
            .find(|o| o.eq_ignore_ascii_case(v))
            .cloned()
            .ok_or_else(|| {
                AppError::BadRequest(format!("not one of the options: {}", options.join(", ")))
            }),
        "date" => {
            let d = NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .map_err(|_| AppError::BadRequest("date must be YYYY-MM-DD".into()))?;
            Ok(d.format("%Y-%m-%d").to_string())
        }
        _ => Err(AppError::BadRequest("unknown field type".into())),
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

// ─── field definitions ──────────────────────────────────────────────────────

pub async fn list(db: &PgPool, project_id: Uuid) -> AppResult<Vec<CustomField>> {
    let rows = sqlx::query_as(
        r#"SELECT id, project_id, name, type, options, created_at
           FROM custom_fields WHERE project_id = $1 ORDER BY lower(name)"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn create(
    db: &PgPool,
    project_id: Uuid,
    name: &str,
    field_type: &str,
    options: &[String],
) -> AppResult<CustomField> {
    sqlx::query_as(
        r#"INSERT INTO custom_fields (id, project_id, name, type, options)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, project_id, name, type, options, created_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(project_id)
    .bind(name)
    .bind(field_type)
    .bind(options)
    .fetch_one(db)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            AppError::Conflict("a field with that name already exists".into())
        } else {
            e.into()
        }
    })
}

/// Rename and/or replace options. The type is immutable — changing it would
/// silently invalidate stored values; delete + recreate instead.
pub async fn update(
    db: &PgPool,
    id: Uuid,
    project_id: Uuid,
    name: Option<&str>,
    options: Option<&[String]>,
) -> AppResult<CustomField> {
    let row: Option<CustomField> = sqlx::query_as(
        r#"UPDATE custom_fields SET name = COALESCE($3, name), options = COALESCE($4, options)
           WHERE id = $1 AND project_id = $2
           RETURNING id, project_id, name, type, options, created_at"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(options)
    .fetch_optional(db)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            AppError::Conflict("a field with that name already exists".into())
        } else {
            e.into()
        }
    })?;
    row.ok_or(AppError::NotFound)
}

pub async fn delete(db: &PgPool, id: Uuid, project_id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM custom_fields WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// The project a field belongs to (for access checks before edit/delete).
pub async fn project_of(db: &PgPool, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar(r#"SELECT project_id FROM custom_fields WHERE id = $1"#)
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn get(db: &PgPool, id: Uuid) -> AppResult<CustomField> {
    sqlx::query_as(
        r#"SELECT id, project_id, name, type, options, created_at
           FROM custom_fields WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)
}

// ─── task values ────────────────────────────────────────────────────────────

/// Every field defined on the project, with this task's value where set.
pub async fn list_for_task(
    db: &PgPool,
    project_id: Uuid,
    task_id: Uuid,
) -> AppResult<Vec<TaskFieldValue>> {
    let rows = sqlx::query_as(
        r#"
        SELECT cf.id      AS field_id,
               cf.name    AS name,
               cf.type    AS type,
               cf.options AS options,
               tfv.value  AS value
        FROM   custom_fields cf
        LEFT JOIN task_field_values tfv
               ON tfv.field_id = cf.id AND tfv.task_id = $2
        WHERE  cf.project_id = $1
        ORDER  BY lower(cf.name)
        "#,
    )
    .bind(project_id)
    .bind(task_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// Upsert one value. `canonical` must already have been through
/// `canonical_value` for the field's type.
pub async fn set_value(
    db: impl sqlx::PgExecutor<'_>,
    task_id: Uuid,
    field_id: Uuid,
    canonical: &str,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO task_field_values (task_id, field_id, value)
        VALUES ($1, $2, $3)
        ON CONFLICT (task_id, field_id)
        DO UPDATE SET value = EXCLUDED.value, updated_at = now()
        "#,
    )
    .bind(task_id)
    .bind(field_id)
    .bind(canonical)
    .execute(db)
    .await?;
    Ok(())
}

/// Clear a value. Idempotent: clearing an unset field is a no-op.
pub async fn clear_value(
    db: impl sqlx::PgExecutor<'_>,
    task_id: Uuid,
    field_id: Uuid,
) -> AppResult<()> {
    sqlx::query(r#"DELETE FROM task_field_values WHERE task_id = $1 AND field_id = $2"#)
        .bind(task_id)
        .bind(field_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Task ids in `project_id` matching ALL of the `(field name, raw value)`
/// pairs. Field names match case-insensitively; values are canonicalised per
/// the field's type before comparing. An unknown field or an unparseable
/// value matches nothing — a filter for a field you don't have shouldn't
/// return the whole board.
pub async fn matching_task_ids(
    db: &PgPool,
    project_id: Uuid,
    pairs: &[(String, String)],
) -> AppResult<HashSet<Uuid>> {
    let mut acc: Option<HashSet<Uuid>> = None;
    for (name, raw) in pairs {
        let field: Option<CustomField> = sqlx::query_as(
            r#"SELECT id, project_id, name, type, options, created_at
               FROM custom_fields WHERE project_id = $1 AND lower(name) = lower($2)"#,
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(db)
        .await?;
        let Some(field) = field else {
            return Ok(HashSet::new());
        };
        let Ok(canonical) = canonical_value(&field.r#type, &field.options, raw) else {
            return Ok(HashSet::new());
        };
        let ids: Vec<Uuid> = sqlx::query_scalar(
            r#"SELECT task_id FROM task_field_values WHERE field_id = $1 AND value = $2"#,
        )
        .bind(field.id)
        .bind(&canonical)
        .fetch_all(db)
        .await?;
        let ids: HashSet<Uuid> = ids.into_iter().collect();
        acc = Some(match acc {
            None => ids,
            Some(prev) => prev.intersection(&ids).copied().collect(),
        });
        if acc.as_ref().is_some_and(HashSet::is_empty) {
            break;
        }
    }
    Ok(acc.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn field_types() {
        for t in FIELD_TYPES {
            assert!(valid_field_type(t));
        }
        assert!(!valid_field_type("checkbox"));
        assert!(!valid_field_type(""));
    }

    #[test]
    fn text_values() {
        assert_eq!(canonical_value("text", &[], "  hi  ").unwrap(), "hi");
        assert!(canonical_value("text", &[], "   ").is_err());
        assert!(canonical_value("text", &[], &"x".repeat(MAX_TEXT_LEN + 1)).is_err());
    }

    #[test]
    fn number_values_canonicalise() {
        assert_eq!(canonical_value("number", &[], "3.50").unwrap(), "3.5");
        assert_eq!(canonical_value("number", &[], "42").unwrap(), "42");
        assert_eq!(canonical_value("number", &[], "-0.25").unwrap(), "-0.25");
        assert!(canonical_value("number", &[], "three").is_err());
        assert!(canonical_value("number", &[], "NaN").is_err());
        assert!(canonical_value("number", &[], "inf").is_err());
    }

    #[test]
    fn select_values_match_case_insensitively() {
        let o = opts(&["Low", "High"]);
        // Stored with the option's spelling, not the caller's.
        assert_eq!(canonical_value("select", &o, "low").unwrap(), "Low");
        assert_eq!(canonical_value("select", &o, "HIGH").unwrap(), "High");
        assert!(canonical_value("select", &o, "medium").is_err());
    }

    #[test]
    fn date_values() {
        assert_eq!(
            canonical_value("date", &[], "2026-06-11").unwrap(),
            "2026-06-11"
        );
        assert!(canonical_value("date", &[], "11/06/2026").is_err());
        assert!(canonical_value("date", &[], "2026-13-40").is_err());
    }
}
