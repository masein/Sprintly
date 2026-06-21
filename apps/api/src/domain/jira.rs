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

use chrono::NaiveDate;
use uuid::Uuid;

use crate::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraComment {
    pub author: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JiraIssue {
    /// Jira "Issue key", e.g. `PROJ-12` — stored as the task's external ref so a
    /// re-import updates this card instead of duplicating it.
    pub key: String,
    pub issue_type: String,
    pub summary: String,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    /// Sub-task parent (Jira "Parent" column).
    pub parent_key: Option<String>,
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
    /// A sub-task if Jira flagged the type as such, or it carries a parent key.
    pub fn is_subtask(&self) -> bool {
        let t = self.issue_type.trim().to_lowercase();
        t == "sub-task" || t == "subtask" || self.parent_key.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct JiraPlan {
    pub issues: Vec<JiraIssue>,
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

/// Jira issue type → one of Sprintly's `feature|bug|chore|spike|incident`. Epics
/// are handled separately (they become Sprintly epics, not tasks); a `Sub-task`
/// is a chore that also gets linked to its parent.
pub fn map_issue_type(jira: &str) -> &'static str {
    match jira.trim().to_lowercase().as_str() {
        "bug" | "defect" => "bug",
        "spike" => "spike",
        "incident" => "incident",
        "task" | "sub-task" | "subtask" | "chore" => "chore",
        // story / epic / new feature / improvement / feature → feature
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

/// Jira CSV comment cell is `created;author;body` (body may itself contain `;`).
/// We keep author + body; an unparseable cell becomes a bodied, author-less note.
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
        })
    } else {
        Some(JiraComment {
            author: None,
            body: cell.to_string(),
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
    let labels_i = idxs_exact(&by, &["labels"]);
    let sprint_i = idxs_exact(&by, &["sprint"]);
    let comment_i = idxs_exact(&by, &["comment"]);
    let due_i = idxs_exact(&by, &["due date"]);
    let parent_i = idxs_exact(&by, &["parent", "parent key", "parent id"]);
    let mut epic_i = idxs_exact(&by, &["epic link", "epic name", "custom field (epic link)"]);
    if epic_i.is_empty() {
        epic_i = idxs_contains(&by, "epic link");
    }
    let sp_i = idxs_contains(&by, "story point");

    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    let mut row = 1; // header is row 1
    for result in rdr.records() {
        row += 1;
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

        issues.push(JiraIssue {
            key,
            issue_type: first(&rec, &type_i).unwrap_or_else(|| "Task".into()),
            summary,
            description: first(&rec, &desc_i).unwrap_or_default(),
            status: first(&rec, &status_i).unwrap_or_else(|| "To do".into()),
            priority: first(&rec, &priority_i),
            labels,
            assignee: first(&rec, &assignee_i),
            parent_key: first(&rec, &parent_i),
            epic: first(&rec, &epic_i),
            // Jira lists every sprint the issue touched; the last is the current.
            sprint: all(&rec, &sprint_i).pop(),
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
    Ok(JiraPlan { issues, warnings })
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
        assert_eq!(map_issue_type("Task"), "chore");
        assert_eq!(map_issue_type("Sub-task"), "chore");
        assert_eq!(map_issue_type("Spike"), "spike");
        assert_eq!(map_issue_type("Incident"), "incident");
        assert_eq!(map_issue_type("Epic"), "feature"); // epics handled separately
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
        let bare = parse_comment("just a note").unwrap();
        assert_eq!(bare.author, None);
        assert_eq!(bare.body, "just a note");
        assert!(parse_comment("   ").is_none());
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
    fn rejects_non_jira_csv() {
        assert!(parse_jira_csv("Name,Status\nx,Done\n").is_err());
    }
}
