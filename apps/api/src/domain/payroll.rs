//! Payroll math.
//!
//! Everything here is pure. Real aggregation queries live in the route layer;
//! these helpers exist so the corner cases (month boundaries, burn-rate
//! thresholds, pay rounding) are testable without a DB.

use chrono::{Datelike, NaiveDate};

/// (first day of month, last day of month) in UTC.
pub fn month_bounds(year: i32, month: u32) -> Option<(NaiveDate, NaiveDate)> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let next_first = NaiveDate::from_ymd_opt(ny, nm, 1)?;
    let last = next_first.pred_opt()?;
    Some((first, last))
}

/// Shift (year, month) by `delta` months.
pub fn month_shift(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let m0 = (year * 12 + (month as i32) - 1) + delta;
    let y = m0.div_euclid(12);
    let m = (m0.rem_euclid(12) + 1) as u32;
    (y, m)
}

/// Pay in cents for `minutes` worked at `rate_cents_per_hour`.
/// Mirror of `timesheets::pay_cents` — duplicated here so the payroll module
/// is independently usable; both stay in sync via shared tests.
pub fn pay_cents(minutes: i64, rate_cents_per_hour: Option<i64>) -> i64 {
    let Some(rate) = rate_cents_per_hour else {
        return 0;
    };
    if rate <= 0 || minutes <= 0 {
        return 0;
    }
    let raw = (minutes as i128) * (rate as i128);
    (raw / 60).clamp(0, i64::MAX as i128) as i64
}

/// Burn-rate status, given how much of a budget has been spent vs the
/// fraction of the period that has elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurnStatus {
    /// No budget set, or budget hasn't been spent at all.
    None,
    /// Spend pace is sustainable for the remainder of the period.
    Ok,
    /// Spend pace is ahead of schedule but not yet over.
    Warn,
    /// Already over budget.
    Over,
}

impl BurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Over => "over",
        }
    }
}

/// `spent_cents` against `budget_cents` over `fraction_elapsed` in [0.0, 1.0].
///
/// Status thresholds:
///   * `spent >= budget`                                       → Over
///   * `spent / budget > fraction_elapsed * 1.10`              → Warn
///   * otherwise                                               → Ok
///
/// Allowing a 10% cushion keeps the warn light from flickering at the start
/// of a month when expensive engineers happen to log their long days early.
pub fn burn_status(
    spent_cents: i64,
    budget_cents: Option<i64>,
    fraction_elapsed: f64,
) -> BurnStatus {
    let Some(budget) = budget_cents else { return BurnStatus::None; };
    if budget <= 0 || spent_cents <= 0 {
        return BurnStatus::Ok;
    }
    if spent_cents >= budget {
        return BurnStatus::Over;
    }
    let ratio = (spent_cents as f64) / (budget as f64);
    let elapsed = fraction_elapsed.clamp(0.0, 1.0);
    if ratio > (elapsed * 1.10) {
        BurnStatus::Warn
    } else {
        BurnStatus::Ok
    }
}

/// Convenience: fraction of the current month elapsed as of `today`.
/// Used by the dashboard burn widget to size the "where we should be" line.
pub fn month_elapsed_fraction(today: NaiveDate) -> f64 {
    let (first, last) = match month_bounds(today.year(), today.month()) {
        Some(v) => v,
        None => return 0.0,
    };
    let total = (last - first).num_days() + 1;
    let so_far = (today - first).num_days() + 1;
    (so_far as f64 / total as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_bounds_jan_dec_leap() {
        let (a, b) = month_bounds(2026, 1).unwrap();
        assert_eq!(a, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(b, NaiveDate::from_ymd_opt(2026, 1, 31).unwrap());
        let (a, b) = month_bounds(2026, 12).unwrap();
        assert_eq!(a, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        assert_eq!(b, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
        // Leap year Feb.
        let (_, b) = month_bounds(2028, 2).unwrap();
        assert_eq!(b, NaiveDate::from_ymd_opt(2028, 2, 29).unwrap());
        // Non-leap Feb.
        let (_, b) = month_bounds(2026, 2).unwrap();
        assert_eq!(b, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }

    #[test]
    fn month_bounds_invalid_month() {
        assert!(month_bounds(2026, 0).is_none());
        assert!(month_bounds(2026, 13).is_none());
    }

    #[test]
    fn month_shift_wraps() {
        assert_eq!(month_shift(2026, 1, -1), (2025, 12));
        assert_eq!(month_shift(2026, 12, 1), (2027, 1));
        assert_eq!(month_shift(2026, 6, 18), (2027, 12));
        assert_eq!(month_shift(2026, 6, -19), (2024, 11));
    }

    #[test]
    fn pay_cents_basics() {
        assert_eq!(pay_cents(60, Some(7500)), 7500);
        assert_eq!(pay_cents(30, Some(7500)), 3750);
        assert_eq!(pay_cents(0, Some(7500)), 0);
        assert_eq!(pay_cents(60, None), 0);
        assert_eq!(pay_cents(60, Some(0)), 0);
    }

    #[test]
    fn burn_no_budget_is_none() {
        assert_eq!(burn_status(100, None, 0.5), BurnStatus::None);
    }

    #[test]
    fn burn_over_takes_priority() {
        assert_eq!(burn_status(150, Some(100), 0.1), BurnStatus::Over);
        assert_eq!(burn_status(100, Some(100), 0.99), BurnStatus::Over);
    }

    #[test]
    fn burn_warn_when_pace_above_elapsed_plus_cushion() {
        // 60% spent at 30% of month → ratio 0.6, threshold 0.33 → Warn.
        assert_eq!(burn_status(60, Some(100), 0.3), BurnStatus::Warn);
    }

    #[test]
    fn burn_ok_when_within_cushion() {
        // 30% spent at 30% of month → exactly at pace → Ok.
        assert_eq!(burn_status(30, Some(100), 0.3), BurnStatus::Ok);
        // 33% spent at 30% of month → within +10% cushion → Ok.
        assert_eq!(burn_status(33, Some(100), 0.3), BurnStatus::Ok);
    }

    #[test]
    fn month_elapsed_fraction_first_and_last() {
        let first = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let last = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let f1 = month_elapsed_fraction(first);
        let f2 = month_elapsed_fraction(last);
        assert!(f1 > 0.0 && f1 < 0.05); // ~1/31
        assert!((f2 - 1.0).abs() < 1e-9);
    }
}
