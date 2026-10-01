//! Sprint KPIs and chart series — progress, scope change, health, and a
//! day-by-day burn series that serves burndown *and* burnup.
//!
//! Pure: the route fetches the sprint, every task that was ever in it, and
//! the sprint's scope history (`sprint_scope_events`, kept by a trigger), and
//! this module turns them into numbers. Keeping it free of SQL is what makes
//! the date arithmetic testable — which is where burndowns go wrong.
//!
//! The rules, so they can be argued with in one place:
//!
//!   * **Membership** at an instant is the task's latest scope event at or
//!     before it: `added` → in, `removed` → out. A task with no history at
//!     all falls back to "in" if it points at the sprint now.
//!   * A sprint is measured **as of** now, or — once completed — a moment
//!     before completion, so the carry-over that completion itself performs
//!     doesn't count as scope removed.
//!   * **Original scope** is membership when the sprint actually started
//!     (`started_at`, else the planned start). Added = tasks that joined after
//!     that and weren't in it; removed = original tasks gone by "as of".
//!   * **Done at an instant** = `completed_at` at or before it. A task
//!     reopened since has no `completed_at`, so it reads as never done — the
//!     honest reading of a reopened task.
//!   * **Unit**: story points if any task in the sprint ever had some,
//!     otherwise task count. A sprint nobody estimated still gets a chart —
//!     it used to draw a flat line at zero, which is the "charts are empty"
//!     report.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StatTask {
    pub id: Uuid,
    pub points: Option<i32>,
    /// Live status — used for "now" counts on open sprints.
    pub status: String,
    pub completed_at: Option<DateTime<Utc>>,
    pub due_date: Option<NaiveDate>,
    /// Has an unfinished task blocking it.
    pub blocked: bool,
    /// `tasks.sprint_id` points at this sprint right now.
    pub currently_in: bool,
}

#[derive(Debug, Clone)]
pub struct ScopeEvent {
    pub task_id: Uuid,
    pub added: bool,
    pub at: DateTime<Utc>,
    pub backfilled: bool,
}

#[derive(Debug, Clone)]
pub struct SprintWindow {
    pub state: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Progress {
    pub tasks_total: i64,
    pub tasks_done: i64,
    pub points_total: i64,
    pub points_done: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ScopeChange {
    pub original_tasks: i64,
    pub added_tasks: i64,
    pub removed_tasks: i64,
    pub original_points: i64,
    pub added_points: i64,
    pub removed_points: i64,
    /// Added ÷ original × 100, in the sprint's unit. `None` with no baseline.
    pub change_percent: Option<f64>,
    /// Leans on history reconstructed when tracking began.
    pub approximate: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Health {
    pub on_track: i64,
    pub at_risk: i64,
    pub blocked: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Days {
    pub elapsed: i64,
    pub total: i64,
}

/// One day of the chart. The `ideal_*` line spans the whole planned window;
/// the measured fields stop at "as of" (later days are `None`, so the chart
/// doesn't draw a flat future it hasn't seen).
#[derive(Debug, Serialize, PartialEq)]
pub struct BurnDay {
    pub date: NaiveDate,
    pub ideal: f64,
    pub remaining: Option<i64>,
    pub done: Option<i64>,
    pub scope: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SprintStats {
    /// "points" or "tasks" — what `series` is measured in.
    pub unit: &'static str,
    pub progress: Progress,
    /// `None` until the sprint has started.
    pub scope: Option<ScopeChange>,
    pub days: Days,
    pub health: Health,
    pub series: Vec<BurnDay>,
}

pub fn compute(
    w: &SprintWindow,
    tasks: &[StatTask],
    events: &[ScopeEvent],
    snapshot_status: Option<&HashMap<Uuid, String>>,
    now: DateTime<Utc>,
) -> SprintStats {
    let mut history: HashMap<Uuid, Vec<&ScopeEvent>> = HashMap::new();
    for e in events {
        history.entry(e.task_id).or_default().push(e);
    }
    for h in history.values_mut() {
        // Stable by time; an add and a remove at the same instant (a task
        // bounced) resolve in insertion order, which is the order they happened.
        h.sort_by_key(|e| e.at);
    }
    let in_at = |t: &StatTask, at: DateTime<Utc>| -> bool {
        match history.get(&t.id) {
            Some(h) => h.iter().rev().find(|e| e.at <= at).is_some_and(|e| e.added),
            None => t.currently_in,
        }
    };

    let as_of = match w.completed_at {
        Some(c) => c - Duration::milliseconds(1),
        None => now,
    };
    let started = w.state != "planned";
    let start = w.started_at.unwrap_or(w.starts_at);

    let has_points = tasks.iter().any(|t| t.points.unwrap_or(0) > 0);
    let unit = if has_points { "points" } else { "tasks" };
    let value = |t: &StatTask| -> i64 {
        if has_points {
            t.points.unwrap_or(0).max(0) as i64
        } else {
            1
        }
    };
    let pts = |t: &StatTask| t.points.unwrap_or(0).max(0) as i64;

    // ── progress, as of ──
    let members: Vec<&StatTask> = tasks.iter().filter(|t| in_at(t, as_of)).collect();
    let is_done_now = |t: &StatTask| match snapshot_status {
        Some(s) => {
            s.get(&t.id)
                .map(String::as_str)
                .unwrap_or(t.status.as_str())
                == "done"
        }
        None => t.status == "done",
    };
    let progress = Progress {
        tasks_total: members.len() as i64,
        tasks_done: members.iter().filter(|t| is_done_now(t)).count() as i64,
        points_total: members.iter().map(|t| pts(t)).sum(),
        points_done: members
            .iter()
            .filter(|t| is_done_now(t))
            .map(|t| pts(t))
            .sum(),
    };

    // ── health of what's still open ──
    let today = now.date_naive();
    let mut health = Health {
        on_track: 0,
        at_risk: 0,
        blocked: 0,
    };
    for t in members.iter().filter(|t| !is_done_now(t)) {
        if t.blocked {
            health.blocked += 1;
        } else if t.due_date.is_some_and(|d| d <= today + Duration::days(2)) {
            health.at_risk += 1;
        } else {
            health.on_track += 1;
        }
    }

    // ── days ──
    let plan_start = w.starts_at.date_naive();
    let plan_end = w.ends_at.date_naive().max(plan_start);
    let total_days = (plan_end - plan_start).num_days() + 1;
    let elapsed = if started {
        ((as_of.date_naive() - plan_start).num_days() + 1).clamp(0, total_days)
    } else {
        0
    };
    let days = Days {
        elapsed,
        total: total_days,
    };

    if !started {
        return SprintStats {
            unit,
            progress,
            scope: None,
            days,
            health,
            series: Vec::new(),
        };
    }

    // ── scope change ──
    let original: HashSet<Uuid> = tasks
        .iter()
        .filter(|t| in_at(t, start))
        .map(|t| t.id)
        .collect();
    let final_: HashSet<Uuid> = members.iter().map(|t| t.id).collect();
    let added: HashSet<Uuid> = events
        .iter()
        .filter(|e| e.added && e.at > start && e.at <= as_of && !original.contains(&e.task_id))
        .map(|e| e.task_id)
        .collect();
    let by_id: HashMap<Uuid, &StatTask> = tasks.iter().map(|t| (t.id, t)).collect();
    let sum = |ids: &mut dyn Iterator<Item = &Uuid>, f: &dyn Fn(&StatTask) -> i64| -> i64 {
        ids.filter_map(|id| by_id.get(id)).map(|t| f(t)).sum()
    };
    let removed: Vec<Uuid> = original
        .iter()
        .filter(|id| !final_.contains(id))
        .copied()
        .collect();
    let original_value = sum(&mut original.iter(), &value);
    let added_value = sum(&mut added.iter(), &value);
    let scope = ScopeChange {
        original_tasks: original.len() as i64,
        added_tasks: added.len() as i64,
        removed_tasks: removed.len() as i64,
        original_points: sum(&mut original.iter(), &pts),
        added_points: sum(&mut added.iter(), &pts),
        removed_points: sum(&mut removed.iter(), &pts),
        change_percent: (original_value > 0)
            .then(|| (added_value as f64 / original_value as f64 * 1000.0).round() / 10.0),
        approximate: events.iter().any(|e| e.backfilled),
    };

    // ── the day grid ──
    // From the planned start (or an earlier actual one): a sprint started
    // late still charts its whole window, and one started today isn't a
    // single invisible point. Days before the actual start simply show the
    // scope, untouched.
    let start_day = start.date_naive().min(plan_start).min(plan_end);
    let cutoff_day = as_of.date_naive();
    // Through the planned end — or further, for a sprint that ran over.
    let grid_end = plan_end.max(cutoff_day);
    let ideal_days = (plan_end - start_day).num_days().max(0) + 1;
    let mut series = Vec::new();
    let mut d = start_day;
    let mut i = 0i64;
    while d <= grid_end {
        let ideal = if d >= plan_end || ideal_days <= 1 {
            0.0
        } else {
            original_value as f64 * (1.0 - i as f64 / (ideal_days - 1) as f64)
        };
        let measured = if d <= cutoff_day {
            // End of that day — or "as of", whichever comes first.
            let eod = d
                .and_hms_milli_opt(23, 59, 59, 999)
                .map(|n| n.and_utc())
                .unwrap_or(as_of);
            let at = eod.min(as_of);
            let mut scope_v = 0;
            let mut done_v = 0;
            for t in tasks.iter().filter(|t| in_at(t, at)) {
                scope_v += value(t);
                if t.completed_at.is_some_and(|c| c <= at) {
                    done_v += value(t);
                }
            }
            Some((scope_v, done_v))
        } else {
            None
        };
        series.push(BurnDay {
            date: d,
            ideal: (ideal * 10.0).round() / 10.0,
            remaining: measured.map(|(s, dn)| s - dn),
            done: measured.map(|(_, dn)| dn),
            scope: measured.map(|(s, _)| s),
        });
        d += Duration::days(1);
        i += 1;
    }

    SprintStats {
        unit,
        progress,
        scope: Some(scope),
        days,
        health,
        series,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, day, hour, 0, 0).unwrap()
    }

    fn task(points: Option<i32>) -> StatTask {
        StatTask {
            id: Uuid::now_v7(),
            points,
            status: "todo".into(),
            completed_at: None,
            due_date: None,
            blocked: false,
            currently_in: true,
        }
    }

    fn added(t: &StatTask, when: DateTime<Utc>) -> ScopeEvent {
        ScopeEvent {
            task_id: t.id,
            added: true,
            at: when,
            backfilled: false,
        }
    }

    fn removed(t: &StatTask, when: DateTime<Utc>) -> ScopeEvent {
        ScopeEvent {
            added: false,
            ..added(t, when)
        }
    }

    fn window() -> SprintWindow {
        // Mon 7 → Fri 11 September, started Monday 9:00.
        SprintWindow {
            state: "active".into(),
            starts_at: at(7, 0),
            ends_at: at(11, 0),
            started_at: Some(at(7, 9)),
            completed_at: None,
        }
    }

    #[test]
    fn points_burn_down_day_by_day_and_stop_at_today() {
        let mut a = task(Some(5));
        let mut b = task(Some(3));
        let c = task(Some(2));
        a.completed_at = Some(at(8, 15));
        a.status = "done".into();
        b.completed_at = Some(at(9, 10));
        b.status = "done".into();
        let ev = vec![
            added(&a, at(6, 12)),
            added(&b, at(6, 12)),
            added(&c, at(6, 12)),
        ];
        let s = compute(&window(), &[a, b, c], &ev, None, at(9, 18));

        assert_eq!(s.unit, "points");
        let rem: Vec<Option<i64>> = s.series.iter().map(|d| d.remaining).collect();
        // Mon 10, Tue 5 (a done), Wed 2 (b done), then nothing measured yet.
        assert_eq!(rem, vec![Some(10), Some(5), Some(2), None, None]);
        // Ideal: 10 → 0 across the five planned days.
        let ideal: Vec<f64> = s.series.iter().map(|d| d.ideal).collect();
        assert_eq!(ideal, vec![10.0, 7.5, 5.0, 2.5, 0.0]);
        assert_eq!(
            s.progress,
            Progress {
                tasks_total: 3,
                tasks_done: 2,
                points_total: 10,
                points_done: 8
            }
        );
        assert_eq!(
            s.days,
            Days {
                elapsed: 3,
                total: 5
            }
        );
    }

    #[test]
    fn an_unestimated_sprint_still_burns_by_task_count() {
        let mut a = task(None);
        let b = task(None);
        a.completed_at = Some(at(8, 12));
        let ev = vec![added(&a, at(6, 0)), added(&b, at(6, 0))];
        let s = compute(&window(), &[a, b], &ev, None, at(8, 20));
        assert_eq!(s.unit, "tasks");
        assert_eq!(s.series[0].remaining, Some(2));
        assert_eq!(s.series[1].remaining, Some(1));
        assert_eq!(s.series[0].ideal, 2.0);
    }

    #[test]
    fn scope_added_after_the_start_is_scope_change() {
        let a = task(Some(4));
        let b = task(Some(4));
        let late = task(Some(2));
        let gone = task(Some(1));
        let ev = vec![
            added(&a, at(6, 0)),
            added(&b, at(6, 0)),
            added(&gone, at(6, 0)),
            added(&late, at(8, 11)),
            removed(&gone, at(9, 9)),
        ];
        let mut gone_now = gone.clone();
        gone_now.currently_in = false;
        let s = compute(&window(), &[a, b, late, gone_now], &ev, None, at(10, 12));
        let sc = s.scope.unwrap();
        assert_eq!(
            (sc.original_tasks, sc.added_tasks, sc.removed_tasks),
            (3, 1, 1)
        );
        assert_eq!(
            (sc.original_points, sc.added_points, sc.removed_points),
            (9, 2, 1)
        );
        // 2 added ÷ 9 original, in points.
        assert_eq!(sc.change_percent, Some(22.2));
        assert!(!sc.approximate);
        // Burnup's scope line moves with it: 9, 9, 11 (late joins Tue), 10 (gone leaves Wed).
        let scope: Vec<Option<i64>> = s.series.iter().take(4).map(|d| d.scope).collect();
        assert_eq!(scope, vec![Some(9), Some(11), Some(10), Some(10)]);
        assert_eq!(s.progress.tasks_total, 3);
    }

    #[test]
    fn completion_carry_over_is_not_scope_removed() {
        let done = {
            let mut t = task(Some(3));
            t.completed_at = Some(at(9, 12));
            t.status = "done".into();
            t
        };
        let mut carried = task(Some(5));
        carried.currently_in = false;
        let mut w = window();
        w.state = "completed".into();
        w.completed_at = Some(at(11, 17));
        let ev = vec![
            added(&done, at(6, 0)),
            added(&carried, at(6, 0)),
            // Completion moved it out, in the same instant it completed.
            removed(&carried, at(11, 17)),
        ];
        let mut snap = HashMap::new();
        snap.insert(done.id, "done".to_string());
        snap.insert(carried.id, "in_progress".to_string());
        let s = compute(&w, &[done, carried], &ev, Some(&snap), at(20, 0));
        let sc = s.scope.unwrap();
        assert_eq!(
            sc.removed_tasks, 0,
            "carry-over happens at completion, not during"
        );
        assert_eq!(
            s.progress,
            Progress {
                tasks_total: 2,
                tasks_done: 1,
                points_total: 8,
                points_done: 3
            }
        );
        // Measured all the way to the last day, nothing after.
        assert_eq!(s.series.last().unwrap().remaining, Some(5));
        assert_eq!(s.days.elapsed, 5);
    }

    #[test]
    fn health_sorts_open_work() {
        let mut blocked = task(None);
        blocked.blocked = true;
        let mut due = task(None);
        due.due_date = Some(NaiveDate::from_ymd_opt(2026, 9, 10).unwrap());
        let fine = task(None);
        let mut finished = task(None);
        finished.status = "done".into();
        let s = compute(
            &window(),
            &[blocked, due, fine, finished],
            &[],
            None,
            at(9, 12),
        );
        assert_eq!(
            s.health,
            Health {
                on_track: 1,
                at_risk: 1,
                blocked: 1
            }
        );
    }

    #[test]
    fn a_planned_sprint_has_no_chart_yet() {
        let mut w = window();
        w.state = "planned".into();
        w.started_at = None;
        let s = compute(&w, &[task(Some(3))], &[], None, at(1, 0));
        assert!(s.series.is_empty());
        assert!(s.scope.is_none());
        assert_eq!(s.days.elapsed, 0);
        assert_eq!(s.progress.points_total, 3);
    }

    #[test]
    fn backfilled_history_is_flagged() {
        let a = task(Some(1));
        let mut e = added(&a, at(6, 0));
        e.backfilled = true;
        let s = compute(&window(), &[a], &[e], None, at(8, 0));
        assert!(s.scope.unwrap().approximate);
    }
}
