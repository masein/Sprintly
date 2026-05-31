//! Timesheet pure logic.
//!
//! Week boundaries: Monday through Sunday, in UTC. We deliberately don't
//! honor the user's timezone for week boundaries in v1 — it makes
//! "submit your timesheet at the end of the week" deterministic across an
//! organisation, and avoids a class of "what week is this entry in" bugs.
//! Per-day display in the UI uses the user's TZ; aggregation here doesn't.
//!
//! Pay math: minutes × hourly_rate_cents / 60. Rounded down. We use BIGINT
//! cents throughout — never floats for money.

use chrono::{Datelike, NaiveDate, Weekday};

/// (Monday, Sunday) of the week containing `d`.
pub fn week_bounds(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    // Weekday::Mon.num_days_from_monday() == 0, etc.
    let offset = d.weekday().num_days_from_monday() as i64;
    let monday = d - chrono::Duration::days(offset);
    let sunday = monday + chrono::Duration::days(6);
    (monday, sunday)
}

/// Adjacent week (delta positive = future, negative = past).
pub fn week_shift(monday: NaiveDate, delta: i64) -> (NaiveDate, NaiveDate) {
    let next = monday + chrono::Duration::days(7 * delta);
    week_bounds(next)
}

/// Pay in cents for `minutes` worked at `rate_cents_per_hour`. Saturating-safe.
pub fn pay_cents(minutes: i64, rate_cents_per_hour: Option<i64>) -> i64 {
    let Some(rate) = rate_cents_per_hour else {
        return 0;
    };
    if rate <= 0 || minutes <= 0 {
        return 0;
    }
    // (minutes * rate) / 60 with saturating-mul for paranoid overflow.
    let raw = (minutes as i128) * (rate as i128);
    (raw / 60).clamp(0, i64::MAX as i128) as i64
}

/// Aggregated totals for a window. Used to snapshot timesheet totals at submit.
#[derive(Debug, Default, Clone, Copy)]
pub struct Totals {
    pub total_minutes: i64,
    pub billable_minutes: i64,
}

impl Totals {
    pub fn add(&mut self, duration_minutes: i64, billable: bool) {
        if duration_minutes <= 0 {
            return;
        }
        self.total_minutes += duration_minutes;
        if billable {
            self.billable_minutes += duration_minutes;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_bounds_monday_is_monday() {
        let mon = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
        assert_eq!(mon.weekday(), Weekday::Mon);
        let (m, s) = week_bounds(mon);
        assert_eq!(m, mon);
        assert_eq!(s, NaiveDate::from_ymd_opt(2026, 5, 31).unwrap());
    }

    #[test]
    fn week_bounds_sunday_finds_preceding_monday() {
        let sun = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let (m, s) = week_bounds(sun);
        assert_eq!(m, NaiveDate::from_ymd_opt(2026, 5, 25).unwrap());
        assert_eq!(s, sun);
    }

    #[test]
    fn week_bounds_wednesday_centers_correctly() {
        let wed = NaiveDate::from_ymd_opt(2026, 5, 27).unwrap();
        let (m, _) = week_bounds(wed);
        assert_eq!(m, NaiveDate::from_ymd_opt(2026, 5, 25).unwrap());
    }

    #[test]
    fn week_shift_forward_and_back() {
        let mon = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
        let (next, _) = week_shift(mon, 1);
        assert_eq!(next, NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        let (prev, _) = week_shift(mon, -1);
        assert_eq!(prev, NaiveDate::from_ymd_opt(2026, 5, 18).unwrap());
    }

    #[test]
    fn pay_cents_basics() {
        // 60 min × $50/h = $50 (5000c).
        assert_eq!(pay_cents(60, Some(5000)), 5000);
        // 30 min × $50/h = $25 (2500c).
        assert_eq!(pay_cents(30, Some(5000)), 2500);
        // No rate → no pay.
        assert_eq!(pay_cents(60, None), 0);
        // Zero or negative minutes/rate → no pay.
        assert_eq!(pay_cents(0, Some(5000)), 0);
        assert_eq!(pay_cents(-10, Some(5000)), 0);
        assert_eq!(pay_cents(60, Some(0)), 0);
        assert_eq!(pay_cents(60, Some(-1)), 0);
    }

    #[test]
    fn totals_skip_invalid_entries() {
        let mut t = Totals::default();
        t.add(60, true);
        t.add(45, false);
        t.add(-5, true);   // skipped
        t.add(0, true);    // skipped
        assert_eq!(t.total_minutes, 105);
        assert_eq!(t.billable_minutes, 60);
    }
}
