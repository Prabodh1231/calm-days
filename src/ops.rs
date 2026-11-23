//! Everything that changes the calendar. Every change goes through the approval gate here.
use crate::approval::{needs_approval, Change, Mv};
use crate::planner::{self, Cand, DEFAULT_HOURS};
use crate::repeat::{self, Repeat};
use crate::store::{from_utc, to_utc, Db, Item, Undo};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};

pub type Res<T> = Result<T, String>;

fn e<T: std::fmt::Display>(x: T) -> String {
    x.to_string()
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Written. The number is the undo-log id.
    Applied(i64),
    /// Not written. Waiting for Approve. The number is the proposal id.
    Proposal(i64),
}

#[derive(Clone, Debug)]
pub struct NewItem {
    pub title: String,
    pub is_event: bool,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub dur_min: i64,
    pub priority: i64,
    pub reminder_min: Option<i64>,
    pub repeat: Option<Repeat>,
    pub until: Option<NaiveDate>,
}

/// When the reminder should fire (UTC seconds). Timed: default 10 minutes before.
/// Date only: 09:00 on that day. `Some(-1)` = no reminder.
pub fn remind_at_for(at: Option<NaiveDateTime>, has_time: bool, reminder_min: Option<i64>) -> Option<i64> {
    let at = at?;
    if reminder_min == Some(-1) {
        return None;
    }
    let t = if has_time {
        at - Duration::minutes(reminder_min.unwrap_or(10).clamp(0, 600_000))
    } else {
        at.date().and_hms_opt(9, 0, 0)? - Duration::minutes(reminder_min.unwrap_or(0).clamp(0, 600_000))
    };
    Some(to_utc(t))
}

fn undo_push(db: &Db, desc: &str, u: &Undo, now: NaiveDateTime) -> Res<i64> {
    db.add_undo(desc, u, to_utc(now)).map_err(e)
}

pub fn create(db: &Db, n: &NewItem, sample: bool) -> Res<Item> {
    let title = n.title.trim();
    if title.is_empty() {
        return Err("Please type a title.".into());
    }
    if title.chars().count() > crate::parser::MAX_TITLE_CHARS {
        return Err("That title is too long (200 letters at most).".into());
    }
    let has_time = n.date.is_some() && n.time.is_some();
    let at_local = n.date.map(|d| d.and_time(n.time.unwrap_or_default()));
    let deadline = at_local.and_then(|a| if has_time { Some(a) } else { a.date().and_hms_opt(23, 59, 59) }).map(to_utc);
    let repeat = if n.date.is_some() { n.repeat.clone() } else { None };
    let mut it = Item {
        id: 0,
        title: title.to_string(),
        kind: if n.is_event { "event" } else { "task" }.into(),
        at: at_local.map(to_utc),
        has_time,
        dur_min: n.dur_min.clamp(5, 24 * 60),
        deadline,
        priority: n.priority.clamp(0, 2),
        reminder_min: n.reminder_min,
        remind_at: remind_at_for(at_local, has_time, n.reminder_min),
        fired: false,
        acked: false,
        repeat,
        until: n.until,
        done: false,
        sample,
    };
    it.id = db.insert(&it).map_err(e)?;
    Ok(it)
}

/// Mark done. A repeating item moves to its next date; the finished one is counted in `completions`.
pub fn complete(db: &Db, id: i64, now: NaiveDateTime) -> Res<i64> {
    let mut it = db.get(id).map_err(e)?.ok_or("That task no longer exists.")?;
    if it.done {
        return Err("Already done.".into());
    }
    let before = it.clone();
    let cid = db.add_completion(&it.title, it.priority, to_utc(now)).map_err(e)?;
    let mut finished = true;
    if let (Some(rule), Some(a)) = (it.repeat.clone(), it.at_local()) {
        let mut next = repeat::next_date(&rule, a.date(), it.until);
        let mut guard = 0;
        while let Some(d) = next {
            let stale = if it.has_time { d.and_time(a.time()) <= now } else { d < now.date() };
            if !stale || guard > 4000 {
                break;
            }
            guard += 1;
            next = repeat::next_date(&rule, d, it.until);
        }
        if let Some(d) = next {
            let na = d.and_time(a.time());
            it.at = Some(to_utc(na));
            it.deadline = Some(if it.has_time { to_utc(na) } else { to_utc(d.and_hms_opt(23, 59, 59).unwrap_or(na)) });
            it.fired = false;
            it.acked = false;
            it.remind_at = remind_at_for(Some(na), it.has_time, it.reminder_min);
            finished = false;
        }
    }
    it.done = finished;
    db.put(&it).map_err(e)?;
    let u = Undo { restore: vec![before], remove_items: vec![], remove_completions: vec![cid] };
    undo_push(db, &format!("Done: {}", it.title), &u, now)
}

fn day_has_fixed_event(items: &[Item], skip: i64, day: NaiveDate) -> bool {
    items.iter().any(|x| x.is_event() && !x.done && x.id != skip && x.at_local().map_or(false, |a| a.date() == day))
}

/// A manual move (drag or "Move to..."). Returns Applied for a single free move, else a Proposal.
pub fn submit_move(db: &Db, moves: &[(i64, NaiveDateTime)], now: NaiveDateTime) -> Res<Outcome> {
    if moves.is_empty() {
        return Err("Nothing to move.".into());
    }
    let items = db.list(false).map_err(e)?;
    let mut mvs = Vec::new();
    for (id, to) in moves {
        let it = items.iter().find(|x| x.id == *id).ok_or("That task no longer exists.")?;
        mvs.push(Mv {
            id: *id,
            title: it.title.clone(),
            from: it.at_local().filter(|_| it.has_time),
            to: *to,
            to_day_has_fixed_event: day_has_fixed_event(&items, *id, to.date()),
        });
    }
    let change = Change::Move { moves: mvs };
    dispatch(db, change, now)
}

pub fn submit_delete(db: &Db, ids: &[i64], now: NaiveDateTime) -> Res<Outcome> {
    let mut titles = Vec::new();
    let (mut events, mut tasks, mut series) = (0, 0, 0);
    let mut good = Vec::new();
    for id in ids {
        if let Some(it) = db.get(*id).map_err(e)? {
            if it.is_event() {
                events += 1;
            } else {
                tasks += 1;
            }
            if it.repeat.is_some() {
                series += 1;
            }
            titles.push(it.title);
            good.push(*id);
        }
    }
    if good.is_empty() {
        return Err("That task no longer exists.".into());
    }
    dispatch(db, Change::Delete { ids: good, titles, events, tasks, recurring_series: series }, now)
}

pub fn submit_edit(db: &Db, id: i64, title: Option<String>, priority: Option<i64>, now: NaiveDateTime) -> Res<Outcome> {
    let it = db.get(id).map_err(e)?.ok_or("That task no longer exists.")?;
    if it.repeat.is_some() {
        return dispatch(db, Change::EditFuture { id, title, priority }, now);
    }
    Ok(Outcome::Applied(apply(db, &Change::EditFuture { id, title, priority }, now)?))
}

/// The gate: ask `needs_approval`; write only when it says no.
fn dispatch(db: &Db, change: Change, now: NaiveDateTime) -> Res<Outcome> {
    if needs_approval(&change) {
        Ok(Outcome::Proposal(db.add_proposal(&change, to_utc(now)).map_err(e)?))
    } else {
        Ok(Outcome::Applied(apply(db, &change, now)?))
    }
}

fn apply_moves(db: &Db, moves: &[Mv], auto: bool, now: NaiveDateTime) -> Res<i64> {
    let mut u = Undo::default();
    for m in moves {
        let Some(mut it) = db.get(m.id).map_err(e)? else { continue };
        u.restore.push(it.clone());
        it.at = Some(to_utc(m.to));
        if !auto && it.has_time {
            it.deadline = it.at; // a manual move of a timed item sets its new time as the deadline
        }
        it.has_time = true;
        it.fired = false;
        it.acked = false;
        it.remind_at = remind_at_for(Some(m.to), true, it.reminder_min);
        db.put(&it).map_err(e)?;
    }
    if u.restore.is_empty() {
        return Err("Those tasks no longer exist.".into());
    }
    let desc = if u.restore.len() == 1 { format!("Moved: {}", u.restore[0].title) } else { format!("Moved {} items", u.restore.len()) };
    undo_push(db, &desc, &u, now)
}

/// Write a change. Callers must have passed the gate (or have an approved proposal).
fn apply(db: &Db, change: &Change, now: NaiveDateTime) -> Res<i64> {
    match change {
        Change::Move { moves } => apply_moves(db, moves, false, now),
        Change::Plan { moves } => apply_moves(db, moves, true, now),
        Change::Delete { ids, .. } => {
            let mut u = Undo::default();
            for id in ids {
                if let Some(it) = db.get(*id).map_err(e)? {
                    u.restore.push(it);
                    db.delete(*id).map_err(e)?;
                }
            }
            let n = u.restore.len();
            undo_push(db, &format!("Deleted {n} item(s)"), &u, now)
        }
        Change::EditFuture { id, title, priority } => {
            let mut it = db.get(*id).map_err(e)?.ok_or("That task no longer exists.")?;
            let before = it.clone();
            if let Some(t) = title.as_ref().map(|t| t.trim()).filter(|t| !t.is_empty()) {
                it.title = t.chars().take(crate::parser::MAX_TITLE_CHARS).collect();
            }
            if let Some(p) = priority {
                it.priority = (*p).clamp(0, 2);
            }
            db.put(&it).map_err(e)?;
            undo_push(db, &format!("Edited: {}", it.title), &Undo { restore: vec![before], ..Undo::default() }, now)
        }
    }
}

pub fn approve(db: &Db, proposal: i64, now: NaiveDateTime) -> Res<i64> {
    let (change, status) = db.get_proposal(proposal).map_err(e)?.ok_or("Proposal not found.")?;
    if status != "pending" {
        return Err("That proposal was already handled.".into());
    }
    let undo = apply(db, &change, now)?;
    db.set_proposal_status(proposal, "approved").map_err(e)?;
    Ok(undo)
}

pub fn cancel(db: &Db, proposal: i64) -> Res<()> {
    db.set_proposal_status(proposal, "cancelled").map_err(e)
}

pub fn undo(db: &Db, id: i64) -> Res<()> {
    let (u, undone) = db.get_undo(id).map_err(e)?.ok_or("Nothing to undo.")?;
    if undone {
        return Err("Already undone.".into());
    }
    for it in &u.restore {
        db.put(it).map_err(e)?;
    }
    for id in &u.remove_items {
        db.delete(*id).map_err(e)?;
    }
    for c in &u.remove_completions {
        db.remove_completion(*c).map_err(e)?;
    }
    db.mark_undone(id).map_err(e)
}

/// "Plan my day" and "Reschedule missed". Only PROPOSES: nothing is written until Approve.
/// Returns the proposal id, or None when there is nothing to place.
pub fn make_plan(db: &Db, now: NaiveDateTime, only_missed: bool) -> Res<Option<i64>> {
    let items = db.list(false).map_err(e)?;
    let cands_src: Vec<&Item> = items
        .iter()
        .filter(|x| {
            !x.is_event()
                && if only_missed { x.is_overdue(now) } else { x.at.is_none() || !x.has_time || x.is_overdue(now) }
        })
        .collect();
    let ids: Vec<i64> = cands_src.iter().map(|x| x.id).collect();
    let busy: Vec<(NaiveDateTime, NaiveDateTime)> = items
        .iter()
        .filter(|x| x.has_time && !ids.contains(&x.id))
        .filter_map(|x| x.at_local().map(|a| (a, a + Duration::minutes(x.dur_min))))
        .collect();
    let cands: Vec<Cand> = cands_src
        .iter()
        .map(|x| Cand { id: x.id, dur: x.dur_min, priority: x.priority, deadline: x.deadline.map(from_utc) })
        .collect();
    let (slots, _) = planner::plan(now, DEFAULT_HOURS, &busy, &cands);
    if slots.is_empty() {
        return Ok(None);
    }
    let moves: Vec<Mv> = slots
        .iter()
        .filter_map(|s| {
            let it = cands_src.iter().find(|x| x.id == s.id)?;
            Some(Mv {
                id: s.id,
                title: it.title.clone(),
                from: it.at_local().filter(|_| it.has_time),
                to: s.start,
                to_day_has_fixed_event: day_has_fixed_event(&items, s.id, s.start.date()),
            })
        })
        .collect();
    // a new plan replaces older pending plans
    for (pid, c) in db.pending_proposals().map_err(e)? {
        if matches!(c, Change::Plan { .. }) {
            db.set_proposal_status(pid, "cancelled").map_err(e)?;
        }
    }
    let change = Change::Plan { moves };
    debug_assert!(needs_approval(&change));
    Ok(Some(db.add_proposal(&change, to_utc(now)).map_err(e)?))
}

/// Example day so the first screen is never empty. Safe to press once.
pub fn load_sample(db: &Db, now: NaiveDateTime) -> Res<usize> {
    if db.list(true).map_err(e)?.iter().any(|x| x.sample) {
        return Err("The sample day is already loaded.".into());
    }
    let today = now.date();
    let day = |n: i64| today + Duration::days(n);
    let t = |h, m| NaiveTime::from_hms_opt(h, m, 0);
    let mk = |title: &str, ev: bool, date: Option<NaiveDate>, time: Option<NaiveTime>, dur, pr, rep: Option<Repeat>| NewItem {
        title: title.into(),
        is_event: ev,
        date,
        time,
        dur_min: dur,
        priority: pr,
        reminder_min: None,
        repeat: rep,
        until: None,
    };
    let list = vec![
        mk("Team standup", true, Some(today), t(10, 0), 30, 1, Some(Repeat::Weekdays)),
        mk("Lunch with Maya", true, Some(today), t(13, 0), 60, 1, None),
        mk("Dentist appointment", true, Some(day(1)), t(16, 30), 60, 1, None),
        mk("Call John", false, Some(day(1)), t(17, 0), 15, 1, None),
        mk("Send project report", false, Some(day(2)), None, 90, 2, None),
        mk("Buy groceries", false, Some(today), None, 45, 0, None),
        mk("Pay electricity bill", false, Some(day(-1)), None, 15, 2, None),
        mk("Water the plants", false, Some(today), t(20, 0), 10, 1, Some(Repeat::Daily(1))),
        mk("Weekly review", false, Some(today + Duration::days((4 - today.weekday().num_days_from_monday() as i64 + 7) % 7)), t(16, 0), 30, 1, Some(Repeat::Weekly { every: 1, days: vec![4] })),
        mk("Read 20 pages", false, None, None, 30, 0, None),
    ];
    let n = list.len();
    for x in &list {
        create(db, x, true)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(8, 0, 0).unwrap()
    }
    fn dt(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }
    fn task(db: &Db, title: &str, date: Option<u32>, h: Option<u32>, ev: bool) -> Item {
        create(
            db,
            &NewItem {
                title: title.into(),
                is_event: ev,
                date: date.map(|d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap()),
                time: h.and_then(|h| NaiveTime::from_hms_opt(h, 0, 0)),
                dur_min: 30,
                priority: 1,
                reminder_min: None,
                repeat: None,
                until: None,
            },
            false,
        )
        .unwrap()
    }

    #[test]
    fn single_free_drag_applies_and_undo_restores() {
        let db = Db::memory().unwrap();
        let a = task(&db, "A", Some(23), Some(15), false);
        let out = submit_move(&db, &[(a.id, dt(23, 17, 0))], now()).unwrap();
        let Outcome::Applied(u) = out else { panic!("should apply") };
        assert_eq!(db.get(a.id).unwrap().unwrap().at_local(), Some(dt(23, 17, 0)));
        undo(&db, u).unwrap();
        assert_eq!(db.get(a.id).unwrap().unwrap().at_local(), Some(dt(23, 15, 0)));
        assert!(undo(&db, u).is_err());
    }

    #[test]
    fn three_moves_wait_for_approval() {
        let db = Db::memory().unwrap();
        let ids: Vec<i64> = (0..3).map(|i| task(&db, &format!("T{i}"), Some(23), Some(15), false).id).collect();
        let moves: Vec<(i64, NaiveDateTime)> = ids.iter().map(|i| (*i, dt(23, 17, 0))).collect();
        let Outcome::Proposal(p) = submit_move(&db, &moves, now()).unwrap() else { panic!("needs approval") };
        assert_eq!(db.get(ids[0]).unwrap().unwrap().at_local(), Some(dt(23, 15, 0))); // unchanged
        approve(&db, p, now()).unwrap();
        assert_eq!(db.get(ids[0]).unwrap().unwrap().at_local(), Some(dt(23, 17, 0)));
        assert!(approve(&db, p, now()).is_err());
    }

    #[test]
    fn moving_onto_a_day_with_a_fixed_event_needs_approval() {
        let db = Db::memory().unwrap();
        task(&db, "Dentist", Some(24), Some(10), true);
        let a = task(&db, "A", Some(23), Some(15), false);
        assert!(matches!(submit_move(&db, &[(a.id, dt(24, 15, 0))], now()).unwrap(), Outcome::Proposal(_)));
    }

    #[test]
    fn planner_output_stays_unapplied_until_approve() {
        let db = Db::memory().unwrap();
        let a = task(&db, "Report", Some(25), None, false);
        let before = db.get(a.id).unwrap().unwrap();
        let p = make_plan(&db, now(), false).unwrap().expect("a proposal");
        assert_eq!(db.get(a.id).unwrap().unwrap(), before, "nothing written yet");
        assert_eq!(db.pending_proposals().unwrap().len(), 1);
        let u = approve(&db, p, now()).unwrap();
        let after = db.get(a.id).unwrap().unwrap();
        assert!(after.has_time);
        assert_eq!(after.at_local(), Some(dt(23, 9, 0)));
        undo(&db, u).unwrap();
        assert_eq!(db.get(a.id).unwrap().unwrap(), before);
    }

    #[test]
    fn cancelled_plan_changes_nothing() {
        let db = Db::memory().unwrap();
        let a = task(&db, "Report", Some(25), None, false);
        let p = make_plan(&db, now(), false).unwrap().unwrap();
        cancel(&db, p).unwrap();
        assert!(approve(&db, p, now()).is_err());
        assert!(!db.get(a.id).unwrap().unwrap().has_time);
    }

    #[test]
    fn deleting_one_task_is_free_but_an_event_or_series_is_gated() {
        let db = Db::memory().unwrap();
        let a = task(&db, "A", Some(23), Some(15), false);
        let ev = task(&db, "Dentist", Some(24), Some(10), true);
        assert!(matches!(submit_delete(&db, &[ev.id], now()).unwrap(), Outcome::Proposal(_)));
        assert!(db.get(ev.id).unwrap().is_some());
        let Outcome::Applied(u) = submit_delete(&db, &[a.id], now()).unwrap() else { panic!() };
        assert!(db.get(a.id).unwrap().is_none());
        undo(&db, u).unwrap();
        assert!(db.get(a.id).unwrap().is_some());
        let mut n = NewItem { title: "Daily".into(), is_event: false, date: Some(now().date()), time: None, dur_min: 15, priority: 1, reminder_min: None, repeat: Some(Repeat::Daily(1)), until: None };
        n.title = "Series".into();
        let s = create(&db, &n, false).unwrap();
        assert!(matches!(submit_delete(&db, &[s.id], now()).unwrap(), Outcome::Proposal(_)));
    }

    #[test]
    fn done_on_repeating_task_moves_to_next_date_and_counts() {
        let db = Db::memory().unwrap();
        let n = NewItem { title: "Water".into(), is_event: false, date: Some(now().date()), time: NaiveTime::from_hms_opt(20, 0, 0), dur_min: 10, priority: 1, reminder_min: None, repeat: Some(Repeat::Daily(1)), until: None };
        let it = create(&db, &n, false).unwrap();
        complete(&db, it.id, now()).unwrap();
        let after = db.get(it.id).unwrap().unwrap();
        assert!(!after.done);
        assert_eq!(after.at_local(), Some(dt(24, 20, 0)));
        assert_eq!(db.completions_between(0, i64::MAX).unwrap().len(), 1);
    }

    #[test]
    fn series_with_until_finishes() {
        let db = Db::memory().unwrap();
        let n = NewItem { title: "Course".into(), is_event: false, date: Some(now().date()), time: None, dur_min: 30, priority: 1, reminder_min: None, repeat: Some(Repeat::Daily(1)), until: Some(now().date()) };
        let it = create(&db, &n, false).unwrap();
        complete(&db, it.id, now()).unwrap();
        assert!(db.get(it.id).unwrap().unwrap().done);
    }

    #[test]
    fn reminders_fire_once_and_can_be_dismissed() {
        let db = Db::memory().unwrap();
        let a = task(&db, "Call", Some(23), Some(15), false); // default: 10 min before
        assert_eq!(db.check_due(to_utc(dt(23, 14, 0))).unwrap(), 0);
        assert_eq!(db.check_due(to_utc(dt(23, 14, 55))).unwrap(), 1);
        assert_eq!(db.check_due(to_utc(dt(23, 15, 30))).unwrap(), 0);
        assert_eq!(db.due_list().unwrap().len(), 1);
        db.ack(&[a.id]).unwrap();
        assert!(db.due_list().unwrap().is_empty());
    }

    #[test]
    fn date_only_reminder_is_nine_am_and_none_is_respected() {
        assert_eq!(remind_at_for(Some(dt(23, 0, 0)), false, None), Some(to_utc(dt(23, 9, 0))));
        assert_eq!(remind_at_for(Some(dt(23, 15, 0)), true, Some(30)), Some(to_utc(dt(23, 14, 30))));
        assert_eq!(remind_at_for(Some(dt(23, 15, 0)), true, Some(-1)), None);
        assert_eq!(remind_at_for(None, false, None), None);
    }

    #[test]
    fn sample_day_loads_once() {
        let db = Db::memory().unwrap();
        assert!(load_sample(&db, now()).unwrap() >= 8);
        assert!(load_sample(&db, now()).is_err());
    }

    #[test]
    fn missed_tasks_get_a_proposal_only_for_overdue_ones() {
        let db = Db::memory().unwrap();
        task(&db, "Future", Some(25), Some(15), false);
        assert!(make_plan(&db, now(), true).unwrap().is_none());
        let late = task(&db, "Late", Some(22), None, false);
        let p = make_plan(&db, now(), true).unwrap().unwrap();
        let (c, _) = db.get_proposal(p).unwrap().unwrap();
        let Change::Plan { moves } = c else { panic!() };
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].id, late.id);
    }
}
