//! Import / export (F16). Two halves, both built on existing tables — no new
//! schema.
//!
//!  • Import: parse an external board (Trello JSON or a simple CSV) into a
//!    neutral [`ImportPlan`] (pure, unit-tested), then [`apply_import`] writes
//!    it into a project's default board — resolving or creating columns and
//!    labels and minting tasks. A dry run does the *whole* resolution inside a
//!    transaction and rolls it back, so the report is exactly what a real run
//!    would do, minus the commit.
//!  • Export: [`export_bundle`] reads a project into a JSON-serialisable bundle
//!    (columns, labels, tasks + comments + an attachments manifest); the bytes
//!    themselves are not included, only metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{AppError, AppResult};

// ─── neutral import model ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Trello,
    Csv,
    Jira,
    Auto,
}

impl ImportFormat {
    pub fn parse(s: &str) -> ImportFormat {
        match s.to_lowercase().as_str() {
            "trello" => ImportFormat::Trello,
            "csv" => ImportFormat::Csv,
            "jira" => ImportFormat::Jira,
            _ => ImportFormat::Auto,
        }
    }
}

/// Resolve the concrete source for some content + a requested format. `Auto`
/// sniffs JSON (Trello) vs CSV, and *upgrades* a CSV that carries Jira's header
/// set to the Jira path. `Csv` does the same upgrade, so a plain `.csv` upload
/// of a Jira "all fields" export still gets the rich importer. `Jira` is forced.
pub fn resolve_format(content: &str, requested: ImportFormat) -> ImportFormat {
    match requested {
        ImportFormat::Trello | ImportFormat::Jira => requested,
        ImportFormat::Csv => {
            if crate::domain::jira::looks_like_jira(content) {
                ImportFormat::Jira
            } else {
                ImportFormat::Csv
            }
        }
        ImportFormat::Auto => {
            let t = content.trim_start();
            if t.starts_with('{') || t.starts_with('[') {
                ImportFormat::Trello
            } else if crate::domain::jira::looks_like_jira(content) {
                ImportFormat::Jira
            } else {
                ImportFormat::Csv
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTask {
    pub title: String,
    pub description: String,
    pub column: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportPlan {
    /// Distinct column names in first-seen order.
    pub columns: Vec<String>,
    pub tasks: Vec<ImportTask>,
    pub warnings: Vec<String>,
}

/// Map an external list/column name to one of our four board categories.
pub fn infer_category(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("done") || n.contains("complete") || n.contains("closed") || n.contains("ship") {
        "done"
    } else if n.contains("review") || n.contains("qa") || n.contains("test") || n.contains("verify")
    {
        "review"
    } else if n.contains("progress")
        || n.contains("doing")
        || n.contains("wip")
        || n.contains("active")
    {
        "in_progress"
    } else {
        "todo"
    }
}

pub fn parse(content: &str, format: ImportFormat) -> AppResult<ImportPlan> {
    let fmt = match format {
        ImportFormat::Auto => {
            let t = content.trim_start();
            if t.starts_with('{') || t.starts_with('[') {
                ImportFormat::Trello
            } else {
                ImportFormat::Csv
            }
        }
        other => other,
    };
    let plan = match fmt {
        ImportFormat::Trello => parse_trello(content)?,
        ImportFormat::Csv => parse_csv(content)?,
        // Jira has its own richer model + apply path (see apply_jira_import);
        // it never flows through this simple-plan parser.
        ImportFormat::Jira => {
            return Err(AppError::BadRequest(
                "Jira imports use the Jira parser, not the simple CSV path".into(),
            ))
        }
        ImportFormat::Auto => unreachable!(),
    };
    if plan.tasks.is_empty() {
        return Err(AppError::BadRequest("no importable cards found".into()));
    }
    Ok(plan)
}

// ── Trello ──

#[derive(Deserialize)]
struct TrelloBoard {
    #[serde(default)]
    lists: Vec<TrelloList>,
    #[serde(default)]
    cards: Vec<TrelloCard>,
}
#[derive(Deserialize)]
struct TrelloList {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    closed: bool,
}
#[derive(Deserialize)]
struct TrelloCard {
    #[serde(default)]
    name: String,
    #[serde(default)]
    desc: String,
    #[serde(default, rename = "idList")]
    id_list: String,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    labels: Vec<TrelloLabel>,
}
#[derive(Deserialize)]
struct TrelloLabel {
    #[serde(default)]
    name: String,
}

fn parse_trello(content: &str) -> AppResult<ImportPlan> {
    let board: TrelloBoard = serde_json::from_str(content)
        .map_err(|_| AppError::BadRequest("invalid Trello JSON".into()))?;

    let mut warnings = Vec::new();
    let list_name: std::collections::HashMap<String, String> = board
        .lists
        .iter()
        .filter(|l| !l.closed)
        .map(|l| {
            (
                l.id.clone(),
                if l.name.is_empty() {
                    "Imported".into()
                } else {
                    l.name.clone()
                },
            )
        })
        .collect();

    let mut columns: Vec<String> = Vec::new();
    let mut tasks = Vec::new();
    for card in board.cards {
        if card.closed {
            warnings.push(format!("skipped archived card: {}", card.name));
            continue;
        }
        if card.name.trim().is_empty() {
            continue;
        }
        let column = list_name
            .get(&card.id_list)
            .cloned()
            .unwrap_or_else(|| "Imported".into());
        if !columns.iter().any(|c| c.eq_ignore_ascii_case(&column)) {
            columns.push(column.clone());
        }
        let labels = card
            .labels
            .into_iter()
            .map(|l| l.name)
            .filter(|n| !n.trim().is_empty())
            .collect();
        tasks.push(ImportTask {
            title: card.name.trim().to_string(),
            description: card.desc,
            column,
            labels,
        });
    }
    Ok(ImportPlan {
        columns,
        tasks,
        warnings,
    })
}

// ── CSV ──

/// Minimal RFC-4180-ish CSV: a header row naming columns, comma-separated,
/// double-quoted fields may contain commas and `""` escapes. Recognised
/// headers (case-insensitive): title|name, description|desc, column|list|status,
/// labels (semicolon- or comma-separated inside a quoted field).
fn parse_csv(content: &str) -> AppResult<ImportPlan> {
    let mut lines = content.lines().filter(|l| !l.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| AppError::BadRequest("empty CSV".into()))?;
    let cols: Vec<String> = split_csv_row(header)
        .into_iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let idx = |names: &[&str]| cols.iter().position(|c| names.contains(&c.as_str()));
    let title_i = idx(&["title", "name", "card", "summary"])
        .ok_or_else(|| AppError::BadRequest("CSV needs a 'title' or 'name' column".into()))?;
    let desc_i = idx(&["description", "desc", "notes"]);
    let col_i = idx(&["column", "list", "status", "stage"]);
    let labels_i = idx(&["labels", "label", "tags"]);

    let mut columns: Vec<String> = Vec::new();
    let mut tasks = Vec::new();
    for line in lines {
        let fields = split_csv_row(line);
        let get = |i: Option<usize>| i.and_then(|i| fields.get(i)).map(|s| s.trim().to_string());
        let Some(title) = get(Some(title_i)).filter(|t| !t.is_empty()) else {
            continue;
        };
        let column = get(col_i)
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "To do".into());
        if !columns.iter().any(|c| c.eq_ignore_ascii_case(&column)) {
            columns.push(column.clone());
        }
        let labels = get(labels_i)
            .map(|s| {
                s.split([';', ','])
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        tasks.push(ImportTask {
            title,
            description: get(desc_i).unwrap_or_default(),
            column,
            labels,
        });
    }
    Ok(ImportPlan {
        columns,
        tasks,
        warnings: Vec::new(),
    })
}

/// Split a single CSV line, honouring double-quoted fields with `""` escapes.
fn split_csv_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => out.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    out.push(field);
    out
}

// ─── apply (import) ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    /// Which importer ran: "trello" | "csv" | "jira".
    pub source: String,
    pub columns_created: Vec<String>,
    pub columns_reused: Vec<String>,
    pub labels_created: Vec<String>,
    /// Jira-only: epics / sprints / custom fields created during the import.
    pub epics_created: Vec<String>,
    pub sprints_created: Vec<String>,
    pub fields_created: Vec<String>,
    pub tasks_created: i64,
    /// Jira-only: existing tasks matched by external ref and updated in place.
    pub tasks_updated: i64,
    pub comments_created: i64,
    pub warnings: Vec<String>,
}

impl ImportReport {
    /// A report skeleton with the Jira-only fields zeroed (used by the simple
    /// Trello/CSV path).
    fn simple(dry_run: bool, source: &str) -> Self {
        ImportReport {
            dry_run,
            source: source.into(),
            columns_created: Vec::new(),
            columns_reused: Vec::new(),
            labels_created: Vec::new(),
            epics_created: Vec::new(),
            sprints_created: Vec::new(),
            fields_created: Vec::new(),
            tasks_created: 0,
            tasks_updated: 0,
            comments_created: 0,
            warnings: Vec::new(),
        }
    }
}

/// Apply a plan to a project's default board. When `dry_run`, all work happens
/// in a transaction that is rolled back, so nothing persists but the report is
/// the real outcome.
pub async fn apply_import(
    db: &PgPool,
    project_id: Uuid,
    board_id: Uuid,
    plan: &ImportPlan,
    dry_run: bool,
) -> AppResult<ImportReport> {
    let mut tx = db.begin().await?;

    // Existing columns on the board, keyed by lowercased name.
    let existing: Vec<(Uuid, String, f64)> = sqlx::query_as(
        r#"SELECT id, name, sort_order FROM board_columns
            WHERE board_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(board_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut col_id: std::collections::HashMap<String, Uuid> = existing
        .iter()
        .map(|(id, name, _)| (name.to_lowercase(), *id))
        .collect();
    let mut next_sort = existing.iter().map(|(_, _, s)| *s).fold(0.0_f64, f64::max) + 1024.0;

    let mut columns_created = Vec::new();
    let mut columns_reused = Vec::new();
    for name in &plan.columns {
        let key = name.to_lowercase();
        if col_id.contains_key(&key) {
            columns_reused.push(name.clone());
            continue;
        }
        let id = Uuid::now_v7();
        sqlx::query(
            r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(id)
        .bind(board_id)
        .bind(name)
        .bind(infer_category(name))
        .bind(next_sort)
        .execute(&mut *tx)
        .await?;
        col_id.insert(key, id);
        columns_created.push(name.clone());
        next_sort += 1024.0;
    }

    // Existing labels (case-insensitive).
    let existing_labels: Vec<String> =
        sqlx::query_scalar("SELECT lower(name) FROM project_labels WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&mut *tx)
            .await?;
    let mut have_label: std::collections::HashSet<String> = existing_labels.into_iter().collect();
    let mut labels_created = Vec::new();

    let mut tasks_created = 0i64;
    let mut next_task_order = 1024.0_f64;
    for task in &plan.tasks {
        // New labels referenced by this card.
        for label in &task.labels {
            let key = label.to_lowercase();
            if !have_label.contains(&key) {
                sqlx::query(
                    r#"INSERT INTO project_labels (id, project_id, name, color)
                       VALUES ($1, $2, $3, '#7c5cff')"#,
                )
                .bind(Uuid::now_v7())
                .bind(project_id)
                .bind(label)
                .execute(&mut *tx)
                .await?;
                have_label.insert(key);
                labels_created.push(label.clone());
            }
        }

        let column = col_id
            .get(&task.column.to_lowercase())
            .copied()
            // Fall back to any existing column if the named one wasn't created.
            .or_else(|| col_id.values().next().copied())
            .ok_or_else(|| {
                AppError::BadRequest("project has no board column to import into".into())
            })?;
        let category = infer_category(&task.column);

        let row = sqlx::query!(
            r#"UPDATE projects SET next_task_seq = next_task_seq + 1
                WHERE id = $1 AND deleted_at IS NULL
              RETURNING key AS "key!: String", next_task_seq - 1 AS "seq!: i64""#,
            project_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
        let key = format!("{}-{}", row.key, row.seq);

        sqlx::query(
            r#"INSERT INTO tasks
                 (id, project_id, board_id, column_id, key, title, description, status,
                  labels, order_in_column)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(board_id)
        .bind(column)
        .bind(&key)
        .bind(&task.title)
        .bind(&task.description)
        .bind(category)
        .bind(&task.labels)
        .bind(next_task_order)
        .execute(&mut *tx)
        .await?;
        tasks_created += 1;
        next_task_order += 1024.0;
    }

    if dry_run {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }

    Ok(ImportReport {
        columns_created,
        columns_reused,
        labels_created,
        tasks_created,
        warnings: plan.warnings.clone(),
        ..ImportReport::simple(dry_run, "trello-or-csv")
    })
}

// ─── apply (Jira) ────────────────────────────────────────────────────────────

/// Apply a parsed Jira export to a project's default board. Mirrors
/// [`apply_import`]'s dry-run-by-rollback contract, but maps the full Jira shape:
/// epics, sprints, sub-tasks, assignees, priority/type, story points (a number
/// custom field), comments, and the issue key as an external ref so a re-import
/// updates rather than duplicates.
pub async fn apply_jira_import(
    db: &PgPool,
    project_id: Uuid,
    board_id: Uuid,
    plan: &crate::domain::jira::JiraPlan,
    dry_run: bool,
) -> AppResult<ImportReport> {
    use crate::domain::jira;
    use std::collections::{HashMap, HashSet};

    let mut tx = db.begin().await?;
    let mut report = ImportReport::simple(dry_run, "jira");
    report.warnings = plan.warnings.clone();

    // Users for assignee matching: (id, lowercased email, lowercased display_name).
    let users: Vec<(Uuid, String, String)> = sqlx::query_as(
        r#"SELECT id, lower(email::text), lower(display_name)
             FROM users WHERE deleted_at IS NULL"#,
    )
    .fetch_all(&mut *tx)
    .await?;

    // ── columns (from issue statuses) ──
    let existing_cols: Vec<(Uuid, String, f64)> = sqlx::query_as(
        r#"SELECT id, name, sort_order FROM board_columns
            WHERE board_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(board_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut col_id: HashMap<String, Uuid> = existing_cols
        .iter()
        .map(|(id, name, _)| (name.to_lowercase(), *id))
        .collect();
    let mut next_sort = existing_cols
        .iter()
        .map(|(_, _, s)| *s)
        .fold(0.0_f64, f64::max)
        + 1024.0;
    let mut seen_status: Vec<String> = Vec::new();
    for issue in plan.issues.iter().filter(|i| !i.is_epic()) {
        let name = if issue.status.trim().is_empty() {
            "To do".to_string()
        } else {
            issue.status.trim().to_string()
        };
        if !seen_status.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            seen_status.push(name.clone());
        }
        let key = name.to_lowercase();
        if col_id.contains_key(&key) {
            if !report
                .columns_reused
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&name))
            {
                report.columns_reused.push(name.clone());
            }
            continue;
        }
        let id = Uuid::now_v7();
        sqlx::query(
            r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(id)
        .bind(board_id)
        .bind(&name)
        .bind(infer_category(&name))
        .bind(next_sort)
        .execute(&mut *tx)
        .await?;
        col_id.insert(key, id);
        report.columns_created.push(name);
        next_sort += 1024.0;
    }
    let fallback_col = *col_id
        .values()
        .next()
        .ok_or_else(|| AppError::BadRequest("project has no board column to import into".into()))?;

    // ── labels ──
    let existing_labels: Vec<String> =
        sqlx::query_scalar("SELECT lower(name) FROM project_labels WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&mut *tx)
            .await?;
    let mut have_label: HashSet<String> = existing_labels.into_iter().collect();
    for issue in &plan.issues {
        for label in &issue.labels {
            let key = label.to_lowercase();
            if have_label.insert(key) {
                sqlx::query(
                    r#"INSERT INTO project_labels (id, project_id, name, color)
                       VALUES ($1, $2, $3, '#7c5cff')"#,
                )
                .bind(Uuid::now_v7())
                .bind(project_id)
                .bind(label)
                .execute(&mut *tx)
                .await?;
                report.labels_created.push(label.clone());
            }
        }
    }

    // ── epics (Epic-type issues + referenced epics) ──
    let existing_epics: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, lower(name) FROM epics WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&mut *tx)
            .await?;
    let mut epic_by_name: HashMap<String, Uuid> =
        existing_epics.into_iter().map(|(id, n)| (n, id)).collect();
    // Jira epic key (e.g. "PROJ-5") → epic uuid, for issues that link by key.
    let mut epic_by_key: HashMap<String, Uuid> = HashMap::new();

    let ensure_epic = |name: &str,
                       epic_by_name: &mut HashMap<String, Uuid>,
                       created: &mut Vec<String>|
     -> Option<Uuid> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(id) = epic_by_name.get(&trimmed.to_lowercase()) {
            return Some(*id);
        }
        let id = Uuid::now_v7();
        epic_by_name.insert(trimmed.to_lowercase(), id);
        created.push(trimmed.to_string());
        Some(id)
    };

    // Epic-type issues define named epics keyed by their Jira key.
    for issue in plan.issues.iter().filter(|i| i.is_epic()) {
        if let Some(id) = ensure_epic(&issue.summary, &mut epic_by_name, &mut report.epics_created)
        {
            epic_by_key.insert(issue.key.clone(), id);
            // Persist new epics (those just minted this run).
            sqlx::query(
                r#"INSERT INTO epics (id, project_id, name, color)
                   VALUES ($1, $2, $3, '#7c5cff')
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(id)
            .bind(project_id)
            .bind(issue.summary.trim())
            .execute(&mut *tx)
            .await?;
        }
    }
    // Issues that reference an epic by name (not a known epic key) create one.
    for issue in plan.issues.iter().filter(|i| !i.is_epic()) {
        if let Some(epic_ref) = issue.epic.as_deref() {
            if epic_by_key.contains_key(epic_ref) {
                continue; // links to a known epic key — resolved in pass 2
            }
            if let Some(id) = ensure_epic(epic_ref, &mut epic_by_name, &mut report.epics_created) {
                sqlx::query(
                    r#"INSERT INTO epics (id, project_id, name, color)
                       VALUES ($1, $2, $3, '#7c5cff')
                       ON CONFLICT DO NOTHING"#,
                )
                .bind(id)
                .bind(project_id)
                .bind(epic_ref.trim())
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    // ── sprints ──
    let existing_sprints: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, lower(name) FROM sprints WHERE project_id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut sprint_by_name: HashMap<String, Uuid> = existing_sprints
        .into_iter()
        .map(|(id, n)| (n, id))
        .collect();
    for issue in &plan.issues {
        if let Some(name) = issue
            .sprint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let key = name.to_lowercase();
            if sprint_by_name.contains_key(&key) {
                continue;
            }
            let id = Uuid::now_v7();
            // Jira CSV gives a sprint name but no reliable window/state — create
            // a planned sprint with a default fortnight; the lead can adjust.
            sqlx::query(
                r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at)
                   VALUES ($1, $2, $3, now(), now() + interval '14 days')"#,
            )
            .bind(id)
            .bind(project_id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
            sprint_by_name.insert(key, id);
            report.sprints_created.push(name.to_string());
        }
    }

    // ── story-points custom field (created once, if any issue carries points) ──
    let needs_points = plan.issues.iter().any(|i| i.story_points.is_some());
    let mut points_field: Option<Uuid> = None;
    if needs_points {
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM custom_fields WHERE project_id = $1 AND lower(name) = 'story points'",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await?;
        points_field = Some(match existing {
            Some(id) => id,
            None => {
                let id = Uuid::now_v7();
                sqlx::query(
                    r#"INSERT INTO custom_fields (id, project_id, name, type, options)
                       VALUES ($1, $2, 'Story Points', 'number', '{}')"#,
                )
                .bind(id)
                .bind(project_id)
                .execute(&mut *tx)
                .await?;
                report.fields_created.push("Story Points".into());
                id
            }
        });
    }

    // ── pass 1: upsert tasks (Epic-type issues become epics, not tasks) ──
    let mut key_to_task: HashMap<String, Uuid> = HashMap::new();
    let mut next_order = 1024.0_f64;
    let mut unmatched: Vec<String> = Vec::new();
    for issue in plan.issues.iter().filter(|i| !i.is_epic()) {
        let column = col_id
            .get(&issue.status.trim().to_lowercase())
            .copied()
            .unwrap_or(fallback_col);
        let category = infer_category(&issue.status);
        let priority = jira::map_priority(issue.priority.as_deref().unwrap_or(""));
        let r#type = jira::map_issue_type(&issue.issue_type);
        let assignee = match issue.assignee.as_deref() {
            Some(raw) if !raw.trim().is_empty() => {
                let m = jira::match_user(raw, &users);
                if m.is_none() && !unmatched.iter().any(|u| u.eq_ignore_ascii_case(raw.trim())) {
                    unmatched.push(raw.trim().to_string());
                }
                m
            }
            _ => None,
        };

        // Dedupe by external ref → update in place, else mint a new task.
        let existing: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT id FROM tasks
                WHERE project_id = $1 AND external_ref = $2 AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .bind(&issue.key)
        .fetch_optional(&mut *tx)
        .await?;

        let task_id = if let Some(id) = existing {
            sqlx::query(
                r#"UPDATE tasks SET
                       title = $2, description = $3, status = $4, column_id = $5,
                       priority = $6, type = $7, assignee_id = $8, due_date = $9,
                       labels = $10, updated_at = now()
                     WHERE id = $1"#,
            )
            .bind(id)
            .bind(&issue.summary)
            .bind(&issue.description)
            .bind(category)
            .bind(column)
            .bind(priority)
            .bind(r#type)
            .bind(assignee)
            .bind(issue.due_date)
            .bind(&issue.labels)
            .execute(&mut *tx)
            .await?;
            report.tasks_updated += 1;
            id
        } else {
            let (proj_key, seq): (String, i64) = sqlx::query_as(
                r#"UPDATE projects SET next_task_seq = next_task_seq + 1
                    WHERE id = $1 AND deleted_at IS NULL
                  RETURNING key, next_task_seq - 1"#,
            )
            .bind(project_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::NotFound)?;
            let key = format!("{proj_key}-{seq}");
            let id = Uuid::now_v7();
            sqlx::query(
                r#"INSERT INTO tasks
                     (id, project_id, board_id, column_id, key, title, description, status,
                      type, priority, assignee_id, due_date, labels, order_in_column,
                      external_ref)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)"#,
            )
            .bind(id)
            .bind(project_id)
            .bind(board_id)
            .bind(column)
            .bind(&key)
            .bind(&issue.summary)
            .bind(&issue.description)
            .bind(category)
            .bind(r#type)
            .bind(priority)
            .bind(assignee)
            .bind(issue.due_date)
            .bind(&issue.labels)
            .bind(next_order)
            .bind(&issue.key)
            .execute(&mut *tx)
            .await?;
            next_order += 1024.0;
            report.tasks_created += 1;

            // Comments only land when the task is first created — keeps a
            // re-import idempotent (no duplicate comment trails).
            for c in &issue.comments {
                let author = c
                    .author
                    .as_deref()
                    .and_then(|a| jira::match_user(a, &users));
                sqlx::query(
                    r#"INSERT INTO task_comments (id, task_id, author_id, body)
                       VALUES ($1, $2, $3, $4)"#,
                )
                .bind(Uuid::now_v7())
                .bind(id)
                .bind(author)
                .bind(&c.body)
                .execute(&mut *tx)
                .await?;
                report.comments_created += 1;
            }
            id
        };
        key_to_task.insert(issue.key.clone(), task_id);

        // Story points → the number custom field (preserves fractional points).
        if let (Some(pts), Some(field)) = (issue.story_points, points_field) {
            let value = if pts.fract() == 0.0 {
                format!("{}", pts as i64)
            } else {
                pts.to_string()
            };
            sqlx::query(
                r#"INSERT INTO task_field_values (task_id, field_id, value, updated_at)
                   VALUES ($1, $2, $3, now())
                   ON CONFLICT (task_id, field_id)
                   DO UPDATE SET value = EXCLUDED.value, updated_at = now()"#,
            )
            .bind(task_id)
            .bind(field)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
    }

    // ── pass 2: parent / epic / sprint links (all task ids now known) ──
    for issue in plan.issues.iter().filter(|i| !i.is_epic()) {
        let Some(&task_id) = key_to_task.get(&issue.key) else {
            continue;
        };
        // Sub-task parent (only if the parent was itself imported as a task).
        if let Some(parent_key) = issue.parent_key.as_deref() {
            if let Some(&parent_id) = key_to_task.get(parent_key) {
                sqlx::query("UPDATE tasks SET parent_task_id = $2 WHERE id = $1")
                    .bind(task_id)
                    .bind(parent_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        // Epic: by Jira epic key, else by name.
        if let Some(epic_ref) = issue.epic.as_deref() {
            let epic_id = epic_by_key
                .get(epic_ref)
                .copied()
                .or_else(|| epic_by_name.get(&epic_ref.trim().to_lowercase()).copied());
            if let Some(eid) = epic_id {
                sqlx::query("UPDATE tasks SET epic_id = $2 WHERE id = $1")
                    .bind(task_id)
                    .bind(eid)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        // Sprint.
        if let Some(name) = issue
            .sprint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(&sid) = sprint_by_name.get(&name.to_lowercase()) {
                sqlx::query("UPDATE tasks SET sprint_id = $2 WHERE id = $1")
                    .bind(task_id)
                    .bind(sid)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }

    if !unmatched.is_empty() {
        report.warnings.push(format!(
            "{} assignee(s) had no Sprintly match (left unassigned): {}",
            unmatched.len(),
            unmatched.join(", ")
        ));
    }

    if dry_run {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(report)
}

// ─── export ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ExportBundle {
    pub project: ExportProject,
    pub columns: Vec<ExportColumn>,
    pub labels: Vec<ExportLabel>,
    pub tasks: Vec<ExportTask>,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportProject {
    pub key: String,
    pub name: String,
    pub description: String,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportColumn {
    pub name: String,
    pub category: String,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportLabel {
    pub name: String,
    pub color: String,
}
#[derive(Debug, Serialize)]
pub struct ExportTask {
    pub key: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub task_type: String,
    pub priority: String,
    pub column: String,
    pub labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub comments: Vec<ExportComment>,
    pub attachments: Vec<ExportAttachment>,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportComment {
    pub author: Option<String>,
    pub body: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportAttachment {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

pub async fn export_bundle(db: &PgPool, project_id: Uuid) -> AppResult<ExportBundle> {
    let project = sqlx::query_as::<_, ExportProject>(
        "SELECT key, name, description FROM projects WHERE id = $1",
    )
    .bind(project_id)
    .fetch_one(db)
    .await?;

    let columns = sqlx::query_as::<_, ExportColumn>(
        r#"SELECT bc.name, bc.category
             FROM board_columns bc
             JOIN boards b ON b.id = bc.board_id
            WHERE b.project_id = $1 AND bc.deleted_at IS NULL AND b.deleted_at IS NULL
            ORDER BY bc.sort_order"#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let labels = sqlx::query_as::<_, ExportLabel>(
        "SELECT name, color FROM project_labels WHERE project_id = $1 ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    // Tasks + the column they sit in.
    let task_rows = sqlx::query!(
        r#"SELECT t.id           AS "id!: Uuid",
                  t.key          AS "key!: String",
                  t.title        AS "title!: String",
                  t.description  AS "description!: String",
                  t.status       AS "status!: String",
                  t.type         AS "task_type!: String",
                  t.priority     AS "priority!: String",
                  bc.name        AS "column!: String",
                  t.labels       AS "labels!: Vec<String>",
                  t.created_at   AS "created_at!: DateTime<Utc>"
             FROM tasks t
             JOIN board_columns bc ON bc.id = t.column_id
            WHERE t.project_id = $1 AND t.deleted_at IS NULL
            ORDER BY t.key"#,
        project_id
    )
    .fetch_all(db)
    .await?;

    let mut tasks = Vec::with_capacity(task_rows.len());
    for r in task_rows {
        let comments = sqlx::query_as::<_, ExportComment>(
            r#"SELECT u.handle AS author, c.body, c.created_at
                 FROM task_comments c
                 LEFT JOIN users u ON u.id = c.author_id
                WHERE c.task_id = $1 AND c.deleted_at IS NULL
                ORDER BY c.created_at"#,
        )
        .bind(r.id)
        .fetch_all(db)
        .await?;
        let attachments = sqlx::query_as::<_, ExportAttachment>(
            r#"SELECT filename, mime_type, size_bytes, status, created_at
                 FROM task_attachments
                WHERE task_id = $1 AND deleted_at IS NULL
                ORDER BY created_at"#,
        )
        .bind(r.id)
        .fetch_all(db)
        .await?;
        tasks.push(ExportTask {
            key: r.key,
            title: r.title,
            description: r.description,
            status: r.status,
            task_type: r.task_type,
            priority: r.priority,
            column: r.column,
            labels: r.labels,
            created_at: r.created_at,
            comments,
            attachments,
        });
    }

    Ok(ExportBundle {
        project,
        columns,
        labels,
        tasks,
    })
}

/// Flat per-task CSV export.
pub fn export_csv(bundle: &ExportBundle) -> String {
    let mut csv =
        String::from("key,title,status,type,priority,column,labels,comments,attachments\n");
    for t in &bundle.tasks {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            t.key,
            csv_escape(&t.title),
            t.status,
            t.task_type,
            t.priority,
            csv_escape(&t.column),
            csv_escape(&t.labels.join("; ")),
            t.comments.len(),
            t.attachments.len(),
        ));
    }
    csv
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_inference() {
        assert_eq!(infer_category("To Do"), "todo");
        assert_eq!(infer_category("Backlog"), "todo");
        assert_eq!(infer_category("In Progress"), "in_progress");
        assert_eq!(infer_category("Doing"), "in_progress");
        assert_eq!(infer_category("Code Review"), "review");
        assert_eq!(infer_category("QA"), "review");
        assert_eq!(infer_category("Done"), "done");
        assert_eq!(infer_category("Shipped"), "done");
    }

    #[test]
    fn parses_trello_json() {
        let json = r#"{
            "lists": [
                {"id": "l1", "name": "To Do", "closed": false},
                {"id": "l2", "name": "Done", "closed": false},
                {"id": "l3", "name": "Archive", "closed": true}
            ],
            "cards": [
                {"name": "Build it", "desc": "the thing", "idList": "l1",
                 "labels": [{"name": "backend"}, {"name": ""}]},
                {"name": "Ship it", "desc": "", "idList": "l2", "labels": []},
                {"name": "Old card", "idList": "l1", "closed": true}
            ]
        }"#;
        let plan = parse(json, ImportFormat::Auto).unwrap();
        assert_eq!(plan.columns, vec!["To Do", "Done"]);
        assert_eq!(plan.tasks.len(), 2, "archived card skipped");
        assert_eq!(plan.tasks[0].title, "Build it");
        assert_eq!(plan.tasks[0].labels, vec!["backend"]); // empty label dropped
        assert_eq!(plan.tasks[1].column, "Done");
        assert!(plan.warnings.iter().any(|w| w.contains("Old card")));
    }

    #[test]
    fn parses_csv_with_quotes_and_labels() {
        let csv = "Name,Description,List,Labels\n\
                   \"Fix, the bug\",\"He said \"\"hi\"\"\",In Progress,\"backend; urgent\"\n\
                   Plain task,,,\n";
        let plan = parse(csv, ImportFormat::Csv).unwrap();
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].title, "Fix, the bug");
        assert_eq!(plan.tasks[0].description, "He said \"hi\"");
        assert_eq!(plan.tasks[0].column, "In Progress");
        assert_eq!(plan.tasks[0].labels, vec!["backend", "urgent"]);
        // Missing list defaults to "To do".
        assert_eq!(plan.tasks[1].column, "To do");
    }

    #[test]
    fn empty_import_is_rejected() {
        assert!(parse("Name,List\n", ImportFormat::Csv).is_err());
        assert!(parse(r#"{"lists":[],"cards":[]}"#, ImportFormat::Auto).is_err());
    }
}
