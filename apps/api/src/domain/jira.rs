//! Jira "Export Excel CSV (all fields)" → a neutral, richly-typed import plan.
//!
//! This half is **pure** (no DB) and unit-tested: it turns the raw CSV bytes
//! into [`JiraPlan`] and exposes the field mappers (priority/type/user-match/
//! comment/date). The DB-writing half — resolving users, creating epics,
//! sprints, custom fields, and minting/updating tasks — lives in
//! [`crate::domain::import_export::apply_jira_import`].
//!
//! Why a dedicated reader: Jira descriptions and comments routinely contain
//! newlines inside quoted cells, which the line-based CSV path can't handle. We
//! use the `csv` crate (RFC-4180) here. Jira also *repeats* header names
//! (several `Labels`, `Comment`, and `Sprint` columns), so we collect every
//! column that shares a name, not just the first.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;

use crate::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraComment {
    /// Raw Jira author (display name or accountId) — matched to a Sprintly user
    /// where possible, otherwise preserved in the comment body for attribution.
    pub author: Option<String>,
    pub body: String,
    /// When the comment was posted in Jira (the cell's leading timestamp).
    pub created: Option<DateTime<Utc>>,
}

/// A Jira sprint as it appears in the CSV — either a bare name, or the rich
/// `...Sprint@..[id=..,state=CLOSED,name=..,startDate=..,endDate=..]` toString
/// that some exports emit. We carry the window + state when present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraSprint {
    pub name: String,
    /// Lowercased Jira state: `active` | `closed` | `future` (None if unknown).
    pub state: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JiraIssue {
    /// Jira "Issue key", e.g. `PROJ-12` — stored as the task's external ref so a
    /// re-import updates this card instead of duplicating it.
    pub key: String,
    /// Jira numeric "Issue id", e.g. `18740`. Distinct from the key. The
    /// "all fields" export references a parent by this id in the `Parent`
    /// column, so we keep it to resolve `Parent`(id) → that row.
    pub id: Option<String>,
    pub issue_type: String,
    pub summary: String,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    /// Jira repeats "Watchers" columns (one name each); a bare count is dropped.
    pub watchers: Vec<String>,
    /// Parent referenced by *issue key* (the "Parent key" column, e.g.
    /// `CCTV-1740`), or a "Parent" cell that is itself a key (team-managed
    /// exports). Resolved against issue keys directly.
    pub parent_key: Option<String>,
    /// Parent referenced by *numeric issue id* (the "Parent" column, e.g.
    /// `18740`). Resolved by mapping the id back to its row's issue key.
    pub parent_id: Option<String>,
    /// Epic association (Jira "Epic Link" key, or an epic name).
    pub epic: Option<String>,
    /// Sprint name (Jira repeats Sprint columns; we keep the most recent).
    pub sprint: Option<String>,
    pub story_points: Option<f64>,
    pub due_date: Option<NaiveDate>,
    pub comments: Vec<JiraComment>,
}

impl JiraIssue {
    pub fn is_epic(&self) -> bool {
        self.issue_type.trim().eq_ignore_ascii_case("epic")
    }
    /// A sub-task if Jira flagged the type as such, or it carries any parent
    /// reference (by key or by numeric id).
    pub fn is_subtask(&self) -> bool {
        let t = self.issue_type.trim().to_lowercase();
        t == "sub-task" || t == "subtask" || self.parent_key.is_some() || self.parent_id.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct JiraPlan {
    pub issues: Vec<JiraIssue>,
    /// Every distinct sprint seen across the export (richest cell wins), so the
    /// importer can create each with its real window + state.
    pub sprints: Vec<JiraSprint>,
    pub warnings: Vec<String>,
}

// ─── field mappers (pure) ────────────────────────────────────────────────────

/// Jira priority → Sprintly p0–p3. Highest→p0, High→p1, Medium→p2, Low/Lowest→p3,
/// with the usual synonyms; anything unknown lands at the p2 default.
pub fn map_priority(jira: &str) -> &'static str {
    match jira.trim().to_lowercase().as_str() {
        "highest" | "blocker" | "critical" => "p0",
        "high" | "major" => "p1",
        "medium" | "normal" => "p2",
        "low" | "minor" => "p3",
        "lowest" | "trivial" => "p3",
        _ => "p2",
    }
}

/// Jira issue type → one of Sprintly's `feature|bug|chore|spike|incident`.
///
/// Epics are handled separately (they become Sprintly epics, not tasks). A
/// `Sub-task` has no real sub-type in the CSV, so it maps to `chore` and gets
/// its parent link from the hierarchy pass. A generic `Task` maps to `feature`
/// — Sprintly's neutral default — rather than `chore`, since for many teams the
/// Task is the primary work item, not a side errand.
pub fn map_issue_type(jira: &str) -> &'static str {
    match jira.trim().to_lowercase().as_str() {
        "bug" | "defect" => "bug",
        "spike" => "spike",
        "incident" => "incident",
        "sub-task" | "subtask" | "chore" => "chore",
        // story / task / epic / new feature / improvement / feature → feature
        _ => "feature",
    }
}

/// Match a raw Jira assignee (email *or* display name) to a Sprintly user.
/// Email wins, then a case-insensitive display-name match. `users` is
/// `(id, lowercased email, lowercased display_name)`.
pub fn match_user(raw: &str, users: &[(Uuid, String, String)]) -> Option<Uuid> {
    let r = raw.trim().to_lowercase();
    if r.is_empty() {
        return None;
    }
    users
        .iter()
        .find(|(_, email, _)| *email == r)
        .or_else(|| users.iter().find(|(_, _, name)| *name == r))
        .map(|(id, _, _)| *id)
}

/// Derive a Sprintly handle from a raw Jira name/email: lowercase, keep
/// `[a-z0-9]`, and clamp to 3–32 chars. Uniqueness is the caller's job (it
/// appends a suffix on collision). Empty input → "user".
pub fn slug_handle(raw: &str) -> String {
    // If it's an email, slug the local part only.
    let base = raw.split('@').next().unwrap_or(raw);
    let mut s: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(32)
        .collect();
    if s.len() < 3 {
        s = format!("user{s}");
    }
    s
}

/// Jira CSV comment cell is `created;author;body` (body may itself contain `;`).
/// We keep the timestamp + author + body; an unparseable cell becomes a bodied,
/// author-less note.
pub fn parse_comment(cell: &str) -> Option<JiraComment> {
    let cell = cell.trim();
    if cell.is_empty() {
        return None;
    }
    let parts: Vec<&str> = cell.splitn(3, ';').collect();
    if parts.len() == 3 {
        let author = parts[1].trim();
        let body = parts[2].trim();
        if body.is_empty() {
            return None;
        }
        Some(JiraComment {
            author: (!author.is_empty()).then(|| author.to_string()),
            body: body.to_string(),
            created: parse_datetime(parts[0]),
        })
    } else {
        Some(JiraComment {
            author: None,
            body: cell.to_string(),
            created: None,
        })
    }
}

/// Parse a Jira date cell. Jira exports dates as `d/MMM/yy[ h:mm AM]` (e.g.
/// `5/Jul/26 3:45 PM`) or ISO `YYYY-MM-DD`; we take the date part only.
pub fn parse_date(cell: &str) -> Option<NaiveDate> {
    let s = cell.trim();
    if s.is_empty() {
        return None;
    }
    let first = s.split_whitespace().next().unwrap_or(s);
    for fmt in ["%Y-%m-%d", "%d/%b/%y", "%d/%b/%Y", "%m/%d/%Y", "%d-%b-%y"] {
        if let Ok(d) = NaiveDate::parse_from_str(first, fmt) {
            return Some(d);
        }
    }
    None
}

/// Parse a Jira timestamp (comment / sprint dates) to UTC. Handles Jira's
/// `d/MMM/yy h:mm AM`, ISO-8601 with offset/`Z`, and bare dates (→ midnight).
pub fn parse_datetime(cell: &str) -> Option<DateTime<Utc>> {
    let s = cell.trim();
    if s.is_empty() {
        return None;
    }
    // ISO-8601 with a timezone (e.g. 2024-01-14T11:00:00.000Z).
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // Naive datetimes (no zone) — assume UTC.
    for fmt in [
        "%d/%b/%y %I:%M %p",
        "%d/%b/%Y %I:%M %p",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%d/%b/%y %H:%M",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&ndt));
        }
    }
    // Date only → midnight UTC.
    parse_date(s).map(|d| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()))
}

/// Parse a Jira "Sprint" cell. Bare names pass through; the rich GreenHopper
/// `...Sprint@..[id=..,state=CLOSED,name=Foo,startDate=..,endDate=..]` toString
/// is unpacked into name + state + window.
pub fn parse_sprint_cell(cell: &str) -> Option<JiraSprint> {
    let cell = cell.trim();
    if cell.is_empty() {
        return None;
    }
    // Rich form: pull the key=value pairs out of the bracketed body.
    if let (Some(open), Some(close)) = (cell.find('['), cell.rfind(']')) {
        if close > open {
            let mut kv: HashMap<&str, &str> = HashMap::new();
            for pair in cell[open + 1..close].split(',') {
                if let Some((k, v)) = pair.split_once('=') {
                    kv.insert(k.trim(), v.trim());
                }
            }
            if let Some(name) = kv.get("name").filter(|n| !n.is_empty() && **n != "<null>") {
                return Some(JiraSprint {
                    name: name.to_string(),
                    state: kv
                        .get("state")
                        .filter(|s| !s.is_empty() && **s != "<null>")
                        .map(|s| s.to_lowercase()),
                    start: kv.get("startDate").and_then(|s| parse_datetime(s)),
                    end: kv.get("endDate").and_then(|s| parse_datetime(s)),
                });
            }
        }
    }
    // Bare name.
    Some(JiraSprint {
        name: cell.to_string(),
        state: None,
        start: None,
        end: None,
    })
}

// ─── CSV parsing ─────────────────────────────────────────────────────────────

/// Does this look like a Jira "all fields" CSV? (Issue key + Issue Type +
/// Summary headers.) Used for auto-detection so a plain `.csv` upload of a Jira
/// export still gets the rich path.
pub fn looks_like_jira(content: &str) -> bool {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());
    if let Ok(headers) = rdr.headers() {
        let lower: Vec<String> = headers.iter().map(|h| h.trim().to_lowercase()).collect();
        let has = |n: &str| lower.iter().any(|h| h == n);
        return has("issue key") && has("issue type") && has("summary");
    }
    false
}

/// Header name (lowercased) → every column index that carries it.
fn header_map(headers: &csv::StringRecord) -> HashMap<String, Vec<usize>> {
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, h) in headers.iter().enumerate() {
        by_name.entry(h.trim().to_lowercase()).or_default().push(i);
    }
    by_name
}

/// All column indices whose header exactly equals any of `names`, in column order.
fn idxs_exact(by_name: &HashMap<String, Vec<usize>>, names: &[&str]) -> Vec<usize> {
    let mut out: Vec<usize> = names
        .iter()
        .filter_map(|n| by_name.get(*n))
        .flatten()
        .copied()
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// All column indices whose header *contains* `needle` (for fuzzy custom-field
/// names like "Custom field (Story Points)").
fn idxs_contains(by_name: &HashMap<String, Vec<usize>>, needle: &str) -> Vec<usize> {
    let mut out: Vec<usize> = by_name
        .iter()
        .filter(|(name, _)| name.contains(needle))
        .flat_map(|(_, idxs)| idxs.iter().copied())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn first(rec: &csv::StringRecord, idxs: &[usize]) -> Option<String> {
    idxs.iter()
        .filter_map(|&i| rec.get(i))
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

fn all(rec: &csv::StringRecord, idxs: &[usize]) -> Vec<String> {
    idxs.iter()
        .filter_map(|&i| rec.get(i))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn parse_jira_csv(content: &str) -> AppResult<JiraPlan> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers = rdr
        .headers()
        .map_err(|e| AppError::BadRequest(format!("could not read Jira CSV header: {e}")))?
        .clone();
    let by = header_map(&headers);

    let key_i = idxs_exact(&by, &["issue key", "key"]);
    if key_i.is_empty() {
        return Err(AppError::BadRequest(
            "not a Jira export — no 'Issue key' column".into(),
        ));
    }
    let type_i = idxs_exact(&by, &["issue type"]);
    let summary_i = idxs_exact(&by, &["summary"]);
    let desc_i = idxs_exact(&by, &["description"]);
    let status_i = idxs_exact(&by, &["status"]);
    let priority_i = idxs_exact(&by, &["priority"]);
    let assignee_i = idxs_exact(&by, &["assignee"]);
    let reporter_i = idxs_exact(&by, &["reporter", "creator"]);
    let watchers_i = idxs_exact(&by, &["watchers", "watcher"]);
    let labels_i = idxs_exact(&by, &["labels"]);
    let sprint_i = idxs_exact(&by, &["sprint"]);
    let comment_i = idxs_exact(&by, &["comment"]);
    let due_i = idxs_exact(&by, &["due date"]);
    let id_i = idxs_exact(&by, &["issue id"]);
    // Jira references a parent two ways in the same export: "Parent key" holds
    // the parent's *issue key* (CCTV-1740); "Parent" (or "Parent id") holds the
    // parent's *numeric issue id* (18740). Keep them apart — matching a numeric
    // id against issue keys is what flattened every sub-task before.
    let parent_key_i = idxs_exact(&by, &["parent key"]);
    let parent_raw_i = idxs_exact(&by, &["parent", "parent id"]);
    let mut epic_i = idxs_exact(&by, &["epic link", "epic name", "custom field (epic link)"]);
    if epic_i.is_empty() {
        epic_i = idxs_contains(&by, "epic link");
    }
    let sp_i = idxs_contains(&by, "story point");

    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    // Distinct sprints across the whole export, richest cell winning per name.
    let mut sprints: HashMap<String, JiraSprint> = HashMap::new();
    // Row numbers for error messages start at 2 — the header is row 1.
    for (row, result) in (2..).zip(rdr.records()) {
        let rec = result
            .map_err(|e| AppError::BadRequest(format!("malformed Jira CSV row {row}: {e}")))?;
        let key = match first(&rec, &key_i) {
            Some(k) => k,
            None => continue, // blank row
        };
        let summary = first(&rec, &summary_i).unwrap_or_default();
        if summary.is_empty() {
            warnings.push(format!("{key}: no Summary — skipped"));
            continue;
        }

        // Labels: each Labels column holds one token; a single cell may also be
        // whitespace-separated. Flatten + dedupe, preserving first-seen order.
        let mut labels: Vec<String> = Vec::new();
        for cell in all(&rec, &labels_i) {
            for tok in cell.split_whitespace() {
                if !labels.iter().any(|l| l.eq_ignore_ascii_case(tok)) {
                    labels.push(tok.to_string());
                }
            }
        }

        let comments = all(&rec, &comment_i)
            .iter()
            .filter_map(|c| parse_comment(c))
            .collect();

        // Sprints: parse every cell, remember the richest per name, and keep the
        // last as this issue's current sprint.
        let mut current_sprint = None;
        for cell in all(&rec, &sprint_i) {
            if let Some(sp) = parse_sprint_cell(&cell) {
                current_sprint = Some(sp.name.clone());
                let lname = sp.name.to_lowercase();
                let incoming_rich = sp.state.is_some() || sp.start.is_some() || sp.end.is_some();
                match sprints.get(&lname) {
                    Some(existing)
                        if existing.state.is_some()
                            || existing.start.is_some()
                            || existing.end.is_some() => {}
                    _ if incoming_rich => {
                        sprints.insert(lname, sp);
                    }
                    _ => {
                        sprints.entry(lname).or_insert(sp);
                    }
                }
            }
        }

        // Parent resolution. "Parent key" is the issue key; "Parent" is usually
        // the numeric issue id, but a team-managed export folds the key into the
        // bare "Parent" cell — so a non-numeric "Parent" is treated as a key.
        let mut parent_key = first(&rec, &parent_key_i);
        let mut parent_id = None;
        if let Some(raw) = first(&rec, &parent_raw_i) {
            if raw.chars().all(|c| c.is_ascii_digit()) {
                parent_id = Some(raw);
            } else if parent_key.is_none() {
                parent_key = Some(raw);
            }
        }

        issues.push(JiraIssue {
            key,
            id: first(&rec, &id_i),
            issue_type: first(&rec, &type_i).unwrap_or_else(|| "Task".into()),
            summary,
            description: first(&rec, &desc_i).unwrap_or_default(),
            status: first(&rec, &status_i).unwrap_or_else(|| "To do".into()),
            priority: first(&rec, &priority_i),
            labels,
            assignee: first(&rec, &assignee_i),
            reporter: first(&rec, &reporter_i),
            // Drop a bare watcher *count* cell (a number); keep the names.
            watchers: all(&rec, &watchers_i)
                .into_iter()
                .filter(|w| w.parse::<i64>().is_err())
                .collect(),
            parent_key,
            parent_id,
            epic: first(&rec, &epic_i),
            sprint: current_sprint,
            story_points: first(&rec, &sp_i).and_then(|s| s.parse::<f64>().ok()),
            due_date: first(&rec, &due_i).and_then(|s| parse_date(&s)),
            comments,
        });
    }

    if issues.is_empty() {
        return Err(AppError::BadRequest(
            "no importable Jira issues found".into(),
        ));
    }
    Ok(JiraPlan {
        issues,
        sprints: sprints.into_values().collect(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_mapping() {
        assert_eq!(map_priority("Highest"), "p0");
        assert_eq!(map_priority("Blocker"), "p0");
        assert_eq!(map_priority("High"), "p1");
        assert_eq!(map_priority("Medium"), "p2");
        assert_eq!(map_priority("Low"), "p3");
        assert_eq!(map_priority("Lowest"), "p3");
        assert_eq!(map_priority("weird"), "p2");
        assert_eq!(map_priority(""), "p2");
    }

    #[test]
    fn type_mapping() {
        assert_eq!(map_issue_type("Story"), "feature");
        assert_eq!(map_issue_type("Bug"), "bug");
        assert_eq!(map_issue_type("Task"), "feature"); // generic Task → neutral default
        assert_eq!(map_issue_type("Sub-task"), "chore"); // no real sub-type in CSV
        assert_eq!(map_issue_type("Spike"), "spike");
        assert_eq!(map_issue_type("Incident"), "incident");
        assert_eq!(map_issue_type("Epic"), "feature"); // epics handled separately
    }

    #[test]
    fn handle_slugging() {
        assert_eq!(slug_handle("Sam Adams"), "samadams");
        assert_eq!(slug_handle("jo.march@x.test"), "jomarch"); // email → local part
        assert_eq!(slug_handle("J"), "userj"); // padded to >= 3 chars
        assert_eq!(slug_handle(""), "user");
        assert!(slug_handle("a-very-long-name-that-keeps-on-going-forever").len() <= 32);
    }

    #[test]
    fn user_match_email_then_name() {
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let users = vec![
            (id1, "sam@x.test".into(), "sam adams".into()),
            (id2, "jo@x.test".into(), "jo march".into()),
        ];
        assert_eq!(match_user("Sam@x.test", &users), Some(id1)); // email, case-insensitive
        assert_eq!(match_user("Jo March", &users), Some(id2)); // display name
        assert_eq!(match_user("nobody", &users), None);
        assert_eq!(match_user("  ", &users), None);
    }

    #[test]
    fn comment_parsing() {
        let c = parse_comment("12/Jan/24 3:45 PM;Sam Adams;Looks good; ship it").unwrap();
        assert_eq!(c.author.as_deref(), Some("Sam Adams"));
        assert_eq!(c.body, "Looks good; ship it"); // body keeps its semicolons
        assert_eq!(
            c.created,
            Some(Utc.with_ymd_and_hms(2024, 1, 12, 15, 45, 0).unwrap())
        );
        let bare = parse_comment("just a note").unwrap();
        assert_eq!(bare.author, None);
        assert_eq!(bare.body, "just a note");
        assert_eq!(bare.created, None);
        assert!(parse_comment("   ").is_none());
    }

    #[test]
    fn sprint_cell_parsing() {
        // Bare name.
        let bare = parse_sprint_cell("Sprint 7").unwrap();
        assert_eq!(bare.name, "Sprint 7");
        assert_eq!(bare.state, None);
        // Rich GreenHopper toString.
        let rich = parse_sprint_cell(
            "com.atlassian.greenhopper.service.sprint.Sprint@1[id=5,rapidViewId=1,state=CLOSED,name=Sprint 7,startDate=2024-01-01T10:00:00.000Z,endDate=2024-01-14T10:00:00.000Z]",
        )
        .unwrap();
        assert_eq!(rich.name, "Sprint 7");
        assert_eq!(rich.state.as_deref(), Some("closed"));
        assert_eq!(
            rich.start,
            Some(Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap())
        );
        assert_eq!(
            rich.end,
            Some(Utc.with_ymd_and_hms(2024, 1, 14, 10, 0, 0).unwrap())
        );
        assert!(parse_sprint_cell("   ").is_none());
    }

    #[test]
    fn date_parsing() {
        assert_eq!(
            parse_date("5/Jul/26"),
            Some(NaiveDate::from_ymd_opt(2026, 7, 5).unwrap())
        );
        assert_eq!(
            parse_date("2026-07-05 3:45 PM"),
            Some(NaiveDate::from_ymd_opt(2026, 7, 5).unwrap())
        );
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("not a date"), None);
    }

    #[test]
    fn detects_jira_headers() {
        assert!(looks_like_jira(
            "Issue Type,Issue key,Summary,Status\nBug,P-1,x,Done\n"
        ));
        assert!(!looks_like_jira("Name,Description,List\nx,,To do\n"));
    }

    #[test]
    fn parses_all_fields_with_repeated_headers_and_multiline() {
        // Two Labels columns + a multi-line quoted Description.
        let csv = "Issue key,Issue Type,Summary,Description,Status,Priority,Assignee,Labels,Labels,Sprint,Story Points,Parent\n\
                   P-1,Story,\"Build it\",\"line one\nline two\",In Progress,High,sam@x.test,backend,urgent,Sprint 1,5,\n\
                   P-2,Sub-task,\"Subtask of 1\",,To Do,Low,Jo March,,,,2,P-1\n";
        let plan = parse_jira_csv(csv).unwrap();
        assert_eq!(plan.issues.len(), 2);
        let p1 = &plan.issues[0];
        assert_eq!(p1.key, "P-1");
        assert_eq!(p1.summary, "Build it");
        assert_eq!(p1.description, "line one\nline two"); // newline survived
        assert_eq!(p1.labels, vec!["backend", "urgent"]); // both Labels columns collected
        assert_eq!(p1.sprint.as_deref(), Some("Sprint 1"));
        assert_eq!(p1.story_points, Some(5.0));
        assert_eq!(p1.priority.as_deref(), Some("High"));
        let p2 = &plan.issues[1];
        assert_eq!(p2.parent_key.as_deref(), Some("P-1"));
        assert!(p2.is_subtask());
        assert!(!p1.is_subtask());
    }

    #[test]
    fn splits_numeric_parent_from_parent_key() {
        // A real "all fields" export: numeric Issue id + numeric Parent + a
        // separate Parent key. The numeric Parent must NOT land in parent_key
        // (that's the bug that flattened every sub-task).
        let csv = "Issue id,Issue key,Issue Type,Summary,Status,Parent,Parent key\n\
                   18000,CCTV-1,Epic,Surveillance,To Do,,\n\
                   18740,CCTV-2,Task,Install cams,In Progress,18000,CCTV-1\n\
                   18999,CCTV-3,Sub-task,Mount bracket,To Do,18740,CCTV-2\n";
        let plan = parse_jira_csv(csv).unwrap();
        let sub = plan.issues.iter().find(|i| i.key == "CCTV-3").unwrap();
        assert_eq!(sub.id.as_deref(), Some("18999"));
        assert_eq!(sub.parent_key.as_deref(), Some("CCTV-2")); // by key, not "18740"
        assert_eq!(sub.parent_id.as_deref(), Some("18740")); // numeric kept for fallback
        assert!(sub.is_subtask());
    }

    #[test]
    fn numeric_only_parent_keeps_id_for_fallback() {
        // No "Parent key" column — only the numeric Parent. parent_key stays
        // empty; the numeric id is retained so the apply can map it to a key.
        let csv = "Issue id,Issue key,Issue Type,Summary,Status,Parent\n\
                   200,CCTV-2,Task,a,To Do,\n\
                   300,CCTV-3,Sub-task,b,To Do,200\n";
        let plan = parse_jira_csv(csv).unwrap();
        let sub = plan.issues.iter().find(|i| i.key == "CCTV-3").unwrap();
        assert_eq!(sub.parent_key, None);
        assert_eq!(sub.parent_id.as_deref(), Some("200"));
        assert!(sub.is_subtask());
    }

    #[test]
    fn team_managed_parent_holds_a_key() {
        // Team-managed export folds the parent *key* into the bare Parent cell.
        let csv = "Issue key,Issue Type,Summary,Status,Parent\n\
                   TM-1,Story,a,To Do,\n\
                   TM-2,Sub-task,b,To Do,TM-1\n";
        let plan = parse_jira_csv(csv).unwrap();
        let sub = plan.issues.iter().find(|i| i.key == "TM-2").unwrap();
        assert_eq!(sub.parent_key.as_deref(), Some("TM-1")); // non-numeric → key
        assert_eq!(sub.parent_id, None);
    }

    #[test]
    fn rejects_non_jira_csv() {
        assert!(parse_jira_csv("Name,Status\nx,Done\n").is_err());
    }
}
