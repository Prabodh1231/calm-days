//! Small own repeat-rule type (no rrule crate).
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};

pub const DAY_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Repeat {
    /// every N days
    Daily(u32),
    Weekdays,
    /// every N weeks on these days (0 = Mon .. 6 = Sun). Empty = same weekday as the start.
    Weekly { every: u32, days: Vec<u8> },
    /// day of month; 0 = "use the start date's day". Short months use their last day.
    Monthly(u32),
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let first = NaiveDate::from_ymd_opt(y, m, 1);
    let next = first.and_then(|d| d.checked_add_months(Months::new(1)));
    match (first, next) {
        (Some(a), Some(b)) => (b - a).num_days() as u32,
        _ => 28,
    }
}

/// Date in (y, m) with the given day, clamped to the month end.
pub fn month_day(y: i32, m: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y, m, day.max(1).min(days_in_month(y, m)))
}

fn wd(d: NaiveDate) -> u8 {
    d.weekday().num_days_from_monday() as u8
}

/// First date on or after `d` that fits the rule.
pub fn first_on_or_after(rule: &Repeat, d: NaiveDate) -> NaiveDate {
    match rule {
        Repeat::Daily(_) => d,
        Repeat::Weekdays => {
            let mut x = d;
            for _ in 0..7 {
                if wd(x) < 5 {
                    return x;
                }
                x = x.succ_opt().unwrap_or(x);
            }
            d
        }
        Repeat::Weekly { days, .. } => {
            if days.is_empty() {
                return d;
            }
            let mut x = d;
            for _ in 0..7 {
                if days.contains(&wd(x)) {
                    return x;
                }
                x = x.succ_opt().unwrap_or(x);
            }
            d
        }
        Repeat::Monthly(day) => {
            let day = if *day == 0 { d.day() } else { *day };
            match month_day(d.year(), d.month(), day) {
                Some(c) if c >= d => c,
                _ => next_month_day(d, day).unwrap_or(d),
            }
        }
    }
}

fn next_month_day(cur: NaiveDate, day: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(cur.year(), cur.month(), 1)?;
    let nm = first.checked_add_months(Months::new(1))?;
    month_day(nm.year(), nm.month(), day)
}

/// The occurrence strictly after `cur`. `None` if past `until` or out of range.
pub fn next_date(rule: &Repeat, cur: NaiveDate, until: Option<NaiveDate>) -> Option<NaiveDate> {
    let n = match rule {
        Repeat::Daily(n) => cur.checked_add_signed(Duration::try_days((*n).clamp(1, 3650) as i64)?),
        Repeat::Weekdays => {
            let mut x = cur;
            let mut found = None;
            for _ in 0..7 {
                x = x.succ_opt()?;
                if wd(x) < 5 {
                    found = Some(x);
                    break;
                }
            }
            found
        }
        Repeat::Weekly { every, days } => {
            let every = (*every).clamp(1, 520) as i64;
            let days = if days.is_empty() { vec![wd(cur)] } else { days.clone() };
            let week0 = cur - Duration::try_days(wd(cur) as i64)?;
            let mut x = cur;
            let mut found = None;
            for _ in 0..(every * 7 + 7) {
                x = x.succ_opt()?;
                let weeks = (x - week0).num_days() / 7;
                if weeks % every == 0 && days.contains(&wd(x)) {
                    found = Some(x);
                    break;
                }
            }
            found
        }
        Repeat::Monthly(day) => next_month_day(cur, if *day == 0 { cur.day() } else { *day }),
    }?;
    match until {
        Some(u) if n > u => None,
        _ => Some(n),
    }
}

fn ordinal(n: u32) -> String {
    let suf = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suf}")
}

/// Plain-words form. The parser can read it back (used by the editable chip).
pub fn describe(rule: &Repeat) -> String {
    match rule {
        Repeat::Daily(1) => "every day".into(),
        Repeat::Daily(n) => format!("every {n} days"),
        Repeat::Weekdays => "every weekday".into(),
        Repeat::Weekly { every, days } => {
            let base = if *every <= 1 { "every week".to_string() } else { format!("every {every} weeks") };
            if days.is_empty() {
                base
            } else {
                let names: Vec<&str> = days.iter().filter_map(|d| DAY_NAMES.get(*d as usize).copied()).collect();
                format!("{base} on {}", names.join(" and "))
            }
        }
        Repeat::Monthly(0) => "every month".into(),
        Repeat::Monthly(d) => format!("every month on the {}", ordinal(*d)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn daily_and_interval() {
        assert_eq!(next_date(&Repeat::Daily(1), d(2026, 12, 31), None), Some(d(2027, 1, 1)));
        assert_eq!(next_date(&Repeat::Daily(3), d(2026, 2, 27), None), Some(d(2026, 3, 2)));
    }

    #[test]
    fn weekdays_skip_weekend() {
        // 2026-11-27 is a Friday
        assert_eq!(next_date(&Repeat::Weekdays, d(2026, 11, 27), None), Some(d(2026, 11, 30)));
    }

    #[test]
    fn weekly_two_days_and_every_two_weeks() {
        let r = Repeat::Weekly { every: 1, days: vec![0, 2] }; // Mon, Wed
        assert_eq!(next_date(&r, d(2026, 11, 23), None), Some(d(2026, 11, 25))); // Mon -> Wed
        assert_eq!(next_date(&r, d(2026, 11, 25), None), Some(d(2026, 11, 30))); // Wed -> Mon
        let r2 = Repeat::Weekly { every: 2, days: vec![] };
        assert_eq!(next_date(&r2, d(2026, 11, 23), None), Some(d(2026, 12, 7)));
    }

    #[test]
    fn monthly_31st_uses_month_end_and_recovers() {
        let r = Repeat::Monthly(31);
        assert_eq!(next_date(&r, d(2026, 1, 31), None), Some(d(2026, 2, 28)));
        assert_eq!(next_date(&r, d(2026, 2, 28), None), Some(d(2026, 3, 31)));
        assert_eq!(next_date(&r, d(2026, 3, 31), None), Some(d(2026, 4, 30)));
        assert_eq!(next_date(&r, d(2028, 1, 31), None), Some(d(2028, 2, 29)));
    }

    #[test]
    fn until_stops_series() {
        let r = Repeat::Daily(1);
        assert_eq!(next_date(&r, d(2026, 12, 20), Some(d(2026, 12, 20))), None);
        assert_eq!(next_date(&r, d(2026, 12, 19), Some(d(2026, 12, 20))), Some(d(2026, 12, 20)));
    }

    #[test]
    fn first_on_or_after_rules() {
        assert_eq!(first_on_or_after(&Repeat::Monthly(15), d(2026, 11, 20)), d(2026, 12, 15));
        assert_eq!(first_on_or_after(&Repeat::Monthly(15), d(2026, 11, 15)), d(2026, 11, 15));
        assert_eq!(first_on_or_after(&Repeat::Weekdays, d(2026, 11, 28)), d(2026, 11, 30));
    }
}
