//! Daily planner and "What should I do now?". Plain scoring rules, no AI. Same input, same output.
//!
//! Score = priority weight (high 30, medium 20, low 10)
//!       + deadline pressure (overdue 60; otherwise 40 minus 6 per full day left, never below 0; no deadline 0).
//! Tasks are placed in score order (ties: earlier deadline, then lower id) into the first free gap
//! inside working hours that fits the task. Fixed events and timed tasks are never overlapped.
//! A placed task whose end is after its deadline is flagged `late`.
use chrono::{Duration, NaiveDate, NaiveDateTime, Timelike};

#[derive(Clone, Copy, Debug)]
pub struct Hours {
    pub start: u32,
    pub end: u32,
}
pub const DEFAULT_HOURS: Hours = Hours { start: 9, end: 18 };
pub const HORIZON_DAYS: i64 = 7;

#[derive(Clone, Debug)]
pub struct Cand {
    pub id: i64,
    pub dur: i64,
    pub priority: i64,
    pub deadline: Option<NaiveDateTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub id: i64,
    pub start: NaiveDateTime,
    pub late: bool,
}

pub fn score(c: &Cand, now: NaiveDateTime) -> i64 {
    let p = match c.priority {
        2 => 30,
        1 => 20,
        _ => 10,
    };
    let d = match c.deadline {
        None => 0,
        Some(dl) if dl < now => 60,
        Some(dl) => (40 - 6 * (dl - now).num_days()).max(0),
    };
    p + d
}

fn ceil15(t: NaiveDateTime) -> NaiveDateTime {
    let t0 = t.with_second(0).and_then(|x| x.with_nanosecond(0)).unwrap_or(t);
    let add = (15 - (t0.minute() % 15)) % 15;
    let extra = if t0 != t && add == 0 { 15 } else { 0 };
    t0 + Duration::minutes((add + extra) as i64)
}

fn at_hour(d: NaiveDate, h: u32) -> NaiveDateTime {
    d.and_hms_opt(h.min(23), 0, 0).unwrap_or_default()
}

/// Returns placed slots (in placement order) and the ids that did not fit.
pub fn plan(
    now: NaiveDateTime,
    hours: Hours,
    busy: &[(NaiveDateTime, NaiveDateTime)],
    cands: &[Cand],
) -> (Vec<Slot>, Vec<i64>) {
    let mut busy: Vec<(NaiveDateTime, NaiveDateTime)> = busy.to_vec();
    let mut order: Vec<&Cand> = cands.iter().collect();
    order.sort_by(|a, b| {
        score(b, now)
            .cmp(&score(a, now))
            .then_with(|| match (a.deadline, b.deadline) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
            .then(a.id.cmp(&b.id))
    });
    let earliest = ceil15(now);
    let mut slots = Vec::new();
    let mut unplaced = Vec::new();
    'cand: for c in order {
        let dur = Duration::minutes(c.dur.clamp(5, 12 * 60));
        let mut day = earliest.date();
        for _ in 0..=HORIZON_DAYS {
            let win_start = at_hour(day, hours.start);
            let win_end = at_hour(day, hours.end);
            let mut t = if win_start > earliest { win_start } else { earliest };
            while t + dur <= win_end {
                match busy.iter().filter(|(s, e)| *s < t + dur && *e > t).map(|(_, e)| *e).max() {
                    Some(end) => t = ceil15(end),
                    None => {
                        busy.push((t, t + dur));
                        slots.push(Slot { id: c.id, start: t, late: c.deadline.map_or(false, |dl| t + dur > dl) });
                        continue 'cand;
                    }
                }
            }
            day = match day.succ_opt() {
                Some(d) => d,
                None => break,
            };
        }
        unplaced.push(c.id);
    }
    (slots, unplaced)
}

#[derive(Clone, Debug)]
pub struct NowTask {
    pub id: i64,
    pub at: Option<NaiveDateTime>,
    pub has_time: bool,
    pub dur: i64,
    pub priority: i64,
}

/// Rank: overdue high priority, due within 2 hours, slot is now, due today (date only),
/// quick wins (15 minutes or less), then any other overdue. `None` means "You're clear".
pub fn do_now(now: NaiveDateTime, tasks: &[NowTask]) -> Option<(i64, String)> {
    let mut best: Option<(u8, i64, NaiveDateTime, i64, String)> = None;
    for t in tasks {
        let mut cand: Option<(u8, String)> = None;
        match (t.at, t.has_time) {
            (Some(a), true) => {
                let end = a + Duration::minutes(t.dur);
                if a <= now && now < end {
                    cand = Some((3, "It is scheduled for right now.".into()));
                } else if a < now {
                    cand = Some(if t.priority == 2 {
                        (1, "Overdue and high priority.".into())
                    } else {
                        (6, "Overdue. Do it now or move it.".into())
                    });
                } else if a - now <= Duration::hours(2) {
                    cand = Some((2, "Due within 2 hours.".into()));
                }
            }
            (Some(a), false) => {
                if a.date() < now.date() {
                    cand = Some(if t.priority == 2 {
                        (1, "Overdue and high priority.".into())
                    } else {
                        (6, "Overdue. Do it now or move it.".into())
                    });
                } else if a.date() == now.date() {
                    cand = Some((4, "Due today.".into()));
                }
            }
            _ => {}
        }
        if cand.is_none() && t.dur <= 15 {
            cand = Some((5, "A quick win: 15 minutes or less.".into()));
        }
        if let Some((rank, why)) = cand {
            let key = (rank, -t.priority, t.at.unwrap_or(now), t.id);
            if best.as_ref().map_or(true, |b| key < (b.0, b.1, b.2, b.3)) {
                best = Some((key.0, key.1, key.2, key.3, why));
            }
        }
    }
    best.map(|b| (b.3, b.4))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }
    fn cand(id: i64, dur: i64, priority: i64, deadline: Option<NaiveDateTime>) -> Cand {
        Cand { id, dur, priority, deadline }
    }

    #[test]
    fn deadline_and_priority_order_slots() {
        let now = dt(23, 8, 0);
        let c = vec![cand(1, 60, 0, None), cand(2, 60, 2, Some(dt(23, 23, 59))), cand(3, 60, 1, Some(dt(25, 23, 59)))];
        let (s, un) = plan(now, DEFAULT_HOURS, &[], &c);
        assert!(un.is_empty());
        let by: Vec<i64> = s.iter().map(|x| x.id).collect();
        assert_eq!(by, vec![2, 3, 1]);
        assert_eq!(s[0].start, dt(23, 9, 0)); // working hours start
        assert_eq!(s[1].start, dt(23, 10, 0));
    }

    #[test]
    fn never_overlaps_fixed_events_or_leaves_hours() {
        let now = dt(23, 8, 0);
        let busy = vec![(dt(23, 9, 0), dt(23, 12, 0)), (dt(23, 13, 0), dt(23, 17, 30))];
        let c = vec![cand(1, 60, 1, None), cand(2, 60, 1, None)];
        let (s, _) = plan(now, DEFAULT_HOURS, &busy, &c);
        assert_eq!(s[0].start, dt(23, 12, 0)); // the 12-13 gap
        assert_eq!(s[1].start, dt(24, 9, 0)); // 17:30-18:00 is too short, so next day
    }

    #[test]
    fn same_input_same_output_and_ties_by_id() {
        let now = dt(23, 8, 0);
        let c = vec![cand(5, 30, 1, None), cand(3, 30, 1, None), cand(4, 30, 1, None)];
        let a = plan(now, DEFAULT_HOURS, &[], &c);
        let rev: Vec<Cand> = c.iter().rev().cloned().collect();
        assert_eq!(a, plan(now, DEFAULT_HOURS, &[], &rev));
        assert_eq!(a.0.iter().map(|s| s.id).collect::<Vec<_>>(), vec![3, 4, 5]);
    }

    #[test]
    fn starts_after_now_and_flags_late() {
        let now = dt(23, 14, 10);
        let c = vec![cand(1, 60, 2, Some(dt(23, 15, 0)))];
        let (s, _) = plan(now, DEFAULT_HOURS, &[], &c);
        assert_eq!(s[0].start, dt(23, 14, 15));
        assert!(s[0].late);
    }

    #[test]
    fn too_long_for_any_day_is_unplaced() {
        let (s, un) = plan(dt(23, 8, 0), DEFAULT_HOURS, &[], &[cand(1, 600, 1, None)]);
        assert!(s.is_empty());
        assert_eq!(un, vec![1]);
    }

    fn nt(id: i64, at: Option<NaiveDateTime>, has_time: bool, dur: i64, priority: i64) -> NowTask {
        NowTask { id, at, has_time, dur, priority }
    }

    #[test]
    fn do_now_ranking() {
        let now = dt(23, 15, 0);
        let overdue_high = nt(1, Some(dt(22, 0, 0)), false, 30, 2);
        let soon = nt(2, Some(dt(23, 16, 0)), true, 30, 1);
        let today = nt(3, Some(dt(23, 0, 0)), false, 30, 2);
        let quick = nt(4, None, false, 10, 1);
        let all = vec![quick.clone(), today.clone(), soon.clone(), overdue_high.clone()];
        assert_eq!(do_now(now, &all).unwrap().0, 1);
        assert_eq!(do_now(now, &all[..3]).unwrap().0, 2);
        assert_eq!(do_now(now, &all[..2]).unwrap().0, 3);
        assert_eq!(do_now(now, &all[..1]).unwrap().0, 4);
        assert!(do_now(now, &[]).is_none());
        assert!(do_now(now, &[nt(9, None, false, 60, 2)]).is_none());
    }
}
