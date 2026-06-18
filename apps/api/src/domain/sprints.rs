//! Sprint state machine + burndown + summary builder.
//!
//! State machine (boring on purpose):
//!
//! ```text
//! planned ──start──▶ active ──complete──▶ completed
//! ```
//!
//! No going back. M10 may add an admin-only `reopen` for completed sprints,
//! but we don't expose it now.
//!
//! Velocity: at completion, sum of story_points across tasks where
//! status = 'done' AND sprint_id = this sprint. Tasks without story points
//! contribute 0; the team writes story points or accepts 0. We snapshot
//! into `sprints.velocity_points` at completion so the metric doesn't drift
//! when historical tasks get edited.

use chrono::{DateTime, NaiveDate, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprintState {
    Planned,
    Active,
    Completed,
}

impl SprintState {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "planned" => Some(Self::Planned),
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Active => "active",
            Self::Completed => "completed",
        }
    }
}

/// Returns Ok(next) if the transition is legal; Err with a message otherwise.
pub fn next_state(current: SprintState, action: &str) -> Result<SprintState, &'static str> {
    match (current, action) {
        (SprintState::Planned, "start") => Ok(SprintState::Active),
        (SprintState::Active, "complete") => Ok(SprintState::Completed),
        (SprintState::Active, "start") => Err("already active"),
        (SprintState::Planned, "complete") => Err("start the sprint first"),
        (SprintState::Completed, _) => Err("sprint is completed"),
        _ => Err("unknown transition"),
    }
}

/// One point on the burndown chart.
#[derive(Debug, Clone, Copy)]
pub struct BurndownPoint {
    pub date: NaiveDate,
    pub remaining_points: i64,
    pub ideal_points: f64,
}

/// Compute burndown from a sprint window + the history of completion events.
///
/// `completions` is a vector of (completed_at, story_points). We iterate
/// the day grid Mon..end_date inclusive; for each day we subtract every
/// completion whose date ≤ that day. The "ideal" line is a straight line
/// from total → 0.
pub fn burndown(
    starts: DateTime<Utc>,
    ends: DateTime<Utc>,
    total_points: i64,
    completions: &[(DateTime<Utc>, i64)],
) -> Vec<BurndownPoint> {
    let start_day = starts.date_naive();
    let end_day = ends.date_naive();
    let days = (end_day - start_day).num_days().max(0) + 1;
    if days <= 0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(days as usize);
    for i in 0..days {
        let day = start_day + chrono::Duration::days(i);
        // Cumulative completion through end-of-day `day`.
        let burned: i64 = completions
            .iter()
            .filter(|(at, _)| at.date_naive() <= day)
            .map(|(_, p)| *p)
            .sum();
        let remaining = (total_points - burned).max(0);
        let ideal = if days == 1 {
            0.0
        } else {
            let progress = i as f64 / (days - 1) as f64;
            (total_points as f64) * (1.0 - progress)
        };
        out.push(BurndownPoint {
            date: day,
            remaining_points: remaining,
            ideal_points: ideal,
        });
    }
    out
}

/// Build a sharable markdown summary for a closed retro.
pub struct RetroSummaryInput<'a> {
    pub sprint_name: &'a str,
    pub sprint_goal: &'a str,
    pub starts: NaiveDate,
    pub ends: NaiveDate,
    pub velocity_points: Option<i64>,
    pub completed_count: i64,
    pub carried_count: i64,
    pub went_well: Vec<&'a str>,
    pub went_poorly: Vec<&'a str>,
    pub action_items: Vec<&'a str>,
    pub kudos: Vec<&'a str>,
}

pub fn retro_summary_markdown(i: &RetroSummaryInput<'_>) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", i.sprint_name));
    if !i.sprint_goal.is_empty() {
        out.push_str(&format!("> **Goal:** {}\n\n", i.sprint_goal));
    }
    out.push_str(&format!(
        "**Window:** {} → {} ({} days)\n\n",
        i.starts,
        i.ends,
        (i.ends - i.starts).num_days() + 1
    ));
    if let Some(v) = i.velocity_points {
        out.push_str(&format!("**Velocity:** {v} pts\n\n"));
    }
    out.push_str(&format!(
        "**Tasks:** {} completed · {} carried\n\n",
        i.completed_count, i.carried_count
    ));
    section(&mut out, "## Went well", &i.went_well);
    section(&mut out, "## Went poorly", &i.went_poorly);
    section(&mut out, "## Action items", &i.action_items);
    section(&mut out, "## Kudos", &i.kudos);
    out
}

fn section(buf: &mut String, heading: &str, items: &[&str]) {
    buf.push_str(heading);
    buf.push_str("\n\n");
    if items.is_empty() {
        buf.push_str("_(nothing recorded)_\n\n");
        return;
    }
    for it in items {
        buf.push_str("- ");
        // Collapse newlines so each note is a single bullet.
        buf.push_str(&it.replace('\n', " "));
        buf.push('\n');
    }
    buf.push('\n');
}

/// Whether `sprint_id` is a live sprint of `project_id`. Used when creating a
/// task directly into a sprint (sprint-detail quick-add) so a task can never
/// land in another project's sprint. Accepts any executor so callers can run it
/// inside the create-task transaction.
pub async fn sprint_belongs_to_project<'e, E>(
    exec: E,
    sprint_id: uuid::Uuid,
    project_id: uuid::Uuid,
) -> crate::AppResult<bool>
where
    E: sqlx::PgExecutor<'e>,
{
    let found: Option<i32> = sqlx::query_scalar(
        r#"SELECT 1 FROM sprints
            WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(sprint_id)
    .bind(project_id)
    .fetch_optional(exec)
    .await?;
    Ok(found.is_some())
}

/// The project's currently active sprint, if any. The state machine permits at
/// most one active sprint per project, but we `LIMIT 1` defensively. Used to
/// scope the board to "active sprint" without the caller knowing the id.
pub async fn active_sprint_id<'e, E>(
    exec: E,
    project_id: uuid::Uuid,
) -> crate::AppResult<Option<uuid::Uuid>>
where
    E: sqlx::PgExecutor<'e>,
{
    let id: Option<uuid::Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM sprints
            WHERE project_id = $1 AND state = 'active' AND deleted_at IS NULL
            LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(exec)
    .await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    #[test]
    fn transitions_legal_path() {
        assert_eq!(
            next_state(SprintState::Planned, "start").unwrap(),
            SprintState::Active
        );
        assert_eq!(
            next_state(SprintState::Active, "complete").unwrap(),
            SprintState::Completed
        );
    }

    #[test]
    fn transitions_blocked_paths() {
        assert!(next_state(SprintState::Planned, "complete").is_err());
        assert!(next_state(SprintState::Active, "start").is_err());
        assert!(next_state(SprintState::Completed, "start").is_err());
        assert!(next_state(SprintState::Completed, "complete").is_err());
    }

    #[test]
    fn burndown_endpoints() {
        let start = Utc.with_ymd_and_hms(2026, 5, 25, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 5, 31, 0, 0, 0).unwrap();
        let series = burndown(start, end, 40, &[]);
        assert_eq!(series.len(), 7);
        assert_eq!(series[0].remaining_points, 40);
        assert_eq!(series.last().unwrap().remaining_points, 40);
        // Ideal line is 40 → 0 across 7 points.
        assert!((series[0].ideal_points - 40.0).abs() < 1e-9);
        assert!((series.last().unwrap().ideal_points - 0.0).abs() < 1e-9);
    }

    #[test]
    fn burndown_subtracts_completions_by_day() {
        let start = Utc.with_ymd_and_hms(2026, 5, 25, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 5, 27, 0, 0, 0).unwrap();
        let comps = [(Utc.with_ymd_and_hms(2026, 5, 26, 10, 0, 0).unwrap(), 5)];
        let series = burndown(start, end, 10, &comps);
        assert_eq!(series.len(), 3);
        // Day 0: nothing burned yet.
        assert_eq!(series[0].remaining_points, 10);
        // Day 1: 5 burned.
        assert_eq!(series[1].remaining_points, 5);
        // Day 2: still 5.
        assert_eq!(series[2].remaining_points, 5);
    }

    #[test]
    fn summary_markdown_well_formed() {
        let starts = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
        let ends = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let md = retro_summary_markdown(&RetroSummaryInput {
            sprint_name: "Sprint 23",
            sprint_goal: "ship the vault",
            starts,
            ends,
            velocity_points: Some(34),
            completed_count: 12,
            carried_count: 2,
            went_well: vec!["pairing"],
            went_poorly: vec!["flaky CI"],
            action_items: vec!["fix CI"],
            kudos: vec!["@mohammad nailed the migration"],
        });
        assert!(md.starts_with("# Sprint 23"));
        assert!(md.contains("Velocity"));
        assert!(md.contains("flaky CI"));
        assert!(md.contains("Action items"));
    }

    #[test]
    fn summary_uses_placeholder_for_empty_section() {
        let starts = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
        let ends = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let md = retro_summary_markdown(&RetroSummaryInput {
            sprint_name: "Empty",
            sprint_goal: "",
            starts,
            ends,
            velocity_points: None,
            completed_count: 0,
            carried_count: 0,
            went_well: vec![],
            went_poorly: vec![],
            action_items: vec![],
            kudos: vec![],
        });
        assert!(md.contains("_(nothing recorded)_"));
    }

    // Keep Datelike import "used" — it's pulled in for downstream callers
    // that compute weekdays from the BurndownPoint dates.
    #[allow(dead_code)]
    fn _datelike_marker(d: NaiveDate) -> u32 {
        d.iso_week().week()
    }
}
