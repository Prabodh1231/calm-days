//! Everything that changes the calendar. Every change goes through the approval gate here.
use crate::approval::{needs_approval, CardMove, Change, Mv};
use crate::planner::{self, Cand, DEFAULT_HOURS};
use crate::repeat::{self, Repeat};
use crate::extra;
use crate::store::{from_utc, to_utc, Check, Db, Item, ListRow, Undo, LABELS};
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

/// Write one line in a card's activity log. A failed log never fails the change itself.
fn log(db: &Db, id: i64, text: &str, now: NaiveDateTime) {
    let _ = db.log_activity(id, text, to_utc(now));
}

fn due_note(at: Option<NaiveDateTime>, has_time: bool) -> String {
    match at {
        None => "Due date removed".into(),
        Some(a) if has_time => format!("Due changed to {}", a.format("%a %-d %b, %-I:%M %p")),
        Some(a) => format!("Due changed to {}", a.format("%a %-d %b")),
    }
}

/// The list new cards land in: the first open list that is not the Inbox and not a Done list.
pub fn default_list(db: &Db) -> Res<i64> {
    let ls = db.lists(false).map_err(e)?;
    let inbox = db.inbox_id();
    ls.iter().find(|l| !l.done_list && Some(l.id) != inbox).or(ls.iter().find(|l| !l.done_list)).or(ls.first()).map(|l| l.id).ok_or_else(|| "There is no list to put the card in.".to_string())
}

/// Quick capture (Ctrl+K, the C key) lands in the Inbox when there is one.
pub fn capture_list(db: &Db) -> Res<i64> {
    match db.inbox_id() {
        Some(i) => Ok(i),
        None => default_list(db),
    }
}

pub fn create(db: &Db, n: &NewItem, sample: bool) -> Res<Item> {
    let list = default_list(db)?;
    create_in(db, n, sample, list)
}

pub fn create_in(db: &Db, n: &NewItem, sample: bool, list_id: i64) -> Res<Item> {
    let list = db.get_list(list_id).map_err(e)?.filter(|l| !l.archived).ok_or("That list no longer exists.")?;
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
        done: list.done_list,
        sample,
        list_id,
        pos: db.cards_in(list_id).map_err(e)?.len() as i64,
        notes: String::new(),
        labels: String::new(),
        checklist: Vec::new(),
        archived: false,
        cycle: String::new(),
    };
    it.id = db.insert(&it).map_err(e)?;
    log(db, it.id, &format!("Created in {}", list.name), crate::store::now_local());
    Ok(it)
}

/// Mark done = move the card to the Done list.
pub fn complete(db: &Db, id: i64, now: NaiveDateTime) -> Res<i64> {
    let it = db.get(id).map_err(e)?.ok_or("That task no longer exists.")?;
    if it.done {
        return Err("Already done.".into());
    }
    let done = db.lists(false).map_err(e)?.into_iter().find(|l| l.done_list).ok_or("There is no Done list. Add one by renaming a list.")?;
    place_cards(db, &[(id, done.id, None)], now)
}

/// The next copy of a repeating card, ready to insert, or None when the series ends.
fn next_copy(it: &Item, now: NaiveDateTime) -> Option<Item> {
    let rule = it.repeat.clone()?;
    let a = it.at_local()?;
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
    let d = next?;
    let na = d.and_time(a.time());
    let mut c = it.clone();
    c.id = 0;
    c.done = false;
    c.at = Some(to_utc(na));
    c.deadline = Some(if it.has_time { to_utc(na) } else { to_utc(d.and_hms_opt(23, 59, 59).unwrap_or(na)) });
    c.fired = false;
    c.acked = false;
    c.remind_at = remind_at_for(Some(na), it.has_time, it.reminder_min);
    c.checklist.iter_mut().for_each(|x| x.d = false);
    Some(c)
}

/// Put cards into lists at a place (None = the end). One undo entry covers everything it touched.
/// A card entering a Done list is finished; a repeating one then spawns its next copy in the first
/// open list. A card leaving a Done list is open again.
pub fn place_cards(db: &Db, moves: &[(i64, i64, Option<usize>)], now: NaiveDateTime) -> Res<i64> {
    let mut u = Undo::default();
    let mut touched: Vec<i64> = Vec::new();
    for (id, to, _) in moves {
        let it = db.get(*id).map_err(e)?.ok_or("That card no longer exists.")?;
        for l in [it.list_id, *to] {
            if !touched.contains(&l) {
                touched.push(l);
            }
        }
    }
    for l in &touched {
        u.restore.extend(db.cards_in(*l).map_err(e)?);
    }
    for (id, to, index) in moves {
        let target = db.get_list(*to).map_err(e)?.filter(|l| !l.archived).ok_or("That list no longer exists.")?;
        let mut it = db.get(*id).map_err(e)?.ok_or("That card no longer exists.")?;
        let from = it.list_id;
        let mut order: Vec<i64> = db.cards_in(target.id).map_err(e)?.iter().map(|c| c.id).filter(|c| c != id).collect();
        let at = index.unwrap_or(order.len()).min(order.len());
        order.insert(at, *id);
        let was_done = it.done;
        it.list_id = target.id;
        it.done = target.done_list;
        if from != target.id {
            let from_name = db.get_list(from).map_err(e)?.map(|l| l.name).unwrap_or_default();
            log(db, it.id, &format!("Moved from {from_name} to {}", target.name), now);
        }
        if was_done && !it.done {
            log(db, it.id, "Reopened", now);
        }
        if it.done && !was_done {
            log(db, it.id, "Completed", now);
            let cid = db.add_completion(&it.title, it.priority, to_utc(now)).map_err(e)?;
            u.remove_completions.push(cid);
            if let Some(mut copy) = next_copy(&it, now) {
                let home = default_list(db)?;
                copy.list_id = home;
                copy.pos = db.cards_in(home).map_err(e)?.len() as i64;
                copy.sample = false;
                u.remove_items.push(db.insert(&copy).map_err(e)?);
                it.repeat = None; // the series lives on in the copy
            }
        }
        db.put(&it).map_err(e)?;
        db.renumber(&order, target.id).map_err(e)?;
        if from != target.id {
            let rest: Vec<i64> = db.cards_in(from).map_err(e)?.iter().map(|c| c.id).collect();
            db.renumber(&rest, from).map_err(e)?;
        }
    }
    let desc = if moves.len() == 1 {
        let c = db.get(moves[0].0).map_err(e)?;
        format!("Moved: {}", c.map(|c| c.title).unwrap_or_default())
    } else {
        format!("Moved {} cards", moves.len())
    };
    undo_push(db, &desc, &u, now)
}

fn card_move(db: &Db, id: i64, to: i64, index: Option<usize>) -> Res<CardMove> {
    let it = db.get(id).map_err(e)?.ok_or("That card no longer exists.")?;
    let from = db.get_list(it.list_id).map_err(e)?.map(|l| l.name).unwrap_or_default();
    let target = db.get_list(to).map_err(e)?.filter(|l| !l.archived).ok_or("That list no longer exists.")?;
    Ok(CardMove { id, title: it.title, from_list: from, to_list: target.name, to_list_id: to, index })
}

/// A drag or "Move to...": one card is applied (with Undo); three or more wait for Approve.
pub fn submit_card_moves(db: &Db, moves: &[(i64, i64, Option<usize>)], now: NaiveDateTime) -> Res<Outcome> {
    if moves.is_empty() {
        return Err("Nothing to move.".into());
    }
    let mut cm = Vec::new();
    for (id, to, index) in moves {
        cm.push(card_move(db, *id, *to, *index)?);
    }
    dispatch(db, Change::MoveCards { moves: cm }, now)
}

/// Move every card of one list to the end of another.
pub fn submit_move_all(db: &Db, from: i64, to: i64, now: NaiveDateTime) -> Res<Outcome> {
    if from == to {
        return Err("Pick a different list.".into());
    }
    let ids: Vec<(i64, i64, Option<usize>)> = db.cards_in(from).map_err(e)?.iter().map(|c| (c.id, to, None)).collect();
    if ids.is_empty() {
        return Err("That list has no cards.".into());
    }
    submit_card_moves(db, &ids, now)
}

pub fn submit_archive_list(db: &Db, id: i64, now: NaiveDateTime) -> Res<Outcome> {
    let l = db.get_list(id).map_err(e)?.filter(|l| !l.archived).ok_or("That list no longer exists.")?;
    let all = db.lists(false).map_err(e)?;
    if all.len() <= 1 {
        return Err("Keep at least one list on the board.".into());
    }
    if l.done_list && all.iter().filter(|x| x.done_list).count() <= 1 {
        return Err("Keep one Done list, so finishing a card still has a place to go.".into());
    }
    if !l.done_list && all.iter().filter(|x| !x.done_list).count() <= 1 {
        return Err("Keep at least one open list for new cards.".into());
    }
    let cards = db.cards_in(id).map_err(e)?.len();
    dispatch(db, Change::ArchiveList { id, name: l.name, cards }, now)
}

fn clean_name(name: &str) -> Res<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Please type a name.".into());
    }
    if n.chars().count() > 60 {
        return Err("That name is too long (60 letters at most).".into());
    }
    Ok(n.to_string())
}

pub fn add_list(db: &Db, name: &str, now: NaiveDateTime) -> Res<i64> {
    let name = clean_name(name)?;
    let pos = db.lists(true).map_err(e)?.iter().map(|l| l.pos).max().unwrap_or(-1) + 1;
    let id = db.put_list(&ListRow { id: 0, name: name.clone(), pos, archived: false, done_list: false }).map_err(e)?;
    undo_push(db, &format!("Added list: {name}"), &Undo { remove_lists: vec![id], ..Undo::default() }, now)
}

pub fn rename_list(db: &Db, id: i64, name: &str, now: NaiveDateTime) -> Res<i64> {
    let name = clean_name(name)?;
    let mut l = db.get_list(id).map_err(e)?.ok_or("That list no longer exists.")?;
    let before = l.clone();
    l.name = name;
    db.put_list(&l).map_err(e)?;
    undo_push(db, &format!("Renamed list: {}", l.name), &Undo { restore_lists: vec![before], ..Undo::default() }, now)
}

/// Put a list at a place among the open lists (0 = first).
pub fn move_list(db: &Db, id: i64, index: usize, now: NaiveDateTime) -> Res<i64> {
    let lists = db.lists(false).map_err(e)?;
    let l = lists.iter().find(|l| l.id == id).ok_or("That list no longer exists.")?.clone();
    let mut order: Vec<ListRow> = lists.iter().filter(|x| x.id != id).cloned().collect();
    let at = index.min(order.len());
    order.insert(at, l.clone());
    let mut u = Undo { restore_lists: lists.clone(), ..Undo::default() };
    for (i, x) in order.iter_mut().enumerate() {
        x.pos = i as i64;
        db.put_list(x).map_err(e)?;
    }
    u.restore_lists.retain(|x| order.iter().any(|y| y.id == x.id));
    undo_push(db, &format!("Moved list: {}", l.name), &u, now)
}

pub fn archive_card(db: &Db, id: i64, now: NaiveDateTime) -> Res<i64> {
    let mut it = db.get(id).map_err(e)?.ok_or("That card no longer exists.")?;
    let before = it.clone();
    it.archived = true;
    db.put(&it).map_err(e)?;
    undo_push(db, &format!("Archived: {}", it.title), &Undo { restore: vec![before], ..Undo::default() }, now)
}

/// Everything the card panel can change. The panel always sends the whole form.
#[derive(Clone, Debug)]
pub struct CardForm {
    pub title: String,
    pub notes: String,
    pub priority: i64,
    pub labels: Vec<String>,
    pub checklist: Vec<Check>,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub reminder_min: Option<i64>,
    pub repeat: Option<Repeat>,
    pub until: Option<NaiveDate>,
    pub is_event: bool,
    pub dur_min: i64,
}

pub fn update_card(db: &Db, id: i64, f: &CardForm, now: NaiveDateTime) -> Res<i64> {
    update_card_ext(db, id, f, None, now)
}

/// Like `update_card`, plus the weekly cycle choice ("this", "next", "none"; "past" and None keep it).
pub fn update_card_ext(db: &Db, id: i64, f: &CardForm, cycle: Option<&str>, now: NaiveDateTime) -> Res<i64> {
    let mut it = db.get(id).map_err(e)?.ok_or("That card no longer exists.")?;
    let before = it.clone();
    let title = f.title.trim();
    if title.is_empty() {
        return Err("Please type a title.".into());
    }
    if title.chars().count() > crate::parser::MAX_TITLE_CHARS {
        return Err("That title is too long (200 letters at most).".into());
    }
    if f.notes.chars().count() > 5000 {
        return Err("The notes are too long (5000 letters at most).".into());
    }
    if f.checklist.len() > 100 || f.checklist.iter().any(|c| c.t.chars().count() > 200) {
        return Err("The checklist is too long.".into());
    }
    let has_time = f.date.is_some() && f.time.is_some();
    let at_local = f.date.map(|d| d.and_time(f.time.unwrap_or_default()));
    it.title = title.to_string();
    it.notes = f.notes.clone();
    it.priority = f.priority.clamp(0, 2);
    it.labels = LABELS.iter().filter(|l| f.labels.iter().any(|x| x == *l)).cloned().collect::<Vec<_>>().join(",");
    it.checklist = f.checklist.iter().filter(|c| !c.t.trim().is_empty()).map(|c| Check { t: c.t.trim().to_string(), d: c.d }).collect();
    it.kind = if f.is_event { "event" } else { "task" }.into();
    it.dur_min = f.dur_min.clamp(5, 24 * 60);
    it.until = f.until;
    it.repeat = if f.date.is_some() { f.repeat.clone() } else { None };
    it.reminder_min = f.reminder_min;
    let new_at = at_local.map(to_utc);
    if new_at != it.at || has_time != it.has_time || f.reminder_min != before.reminder_min {
        it.fired = false;
        it.acked = false;
    }
    it.at = new_at;
    it.has_time = has_time;
    it.deadline = at_local.and_then(|a| if has_time { Some(a) } else { a.date().and_hms_opt(23, 59, 59) }).map(to_utc);
    it.remind_at = remind_at_for(at_local, has_time, f.reminder_min);
    if let Some(v) = cycle.and_then(|c| extra::cycle_value(c, now.date())) {
        it.cycle = v;
    }
    db.put(&it).map_err(e)?;
    if it.at != before.at || it.has_time != before.has_time {
        log(db, id, &due_note(it.at_local(), it.has_time), now);
    }
    for l in it.label_keys() {
        if !before.label_keys().contains(&l) {
            log(db, id, &format!("Label added: {}", extra::label_title(l)), now);
        }
    }
    if it.cycle != before.cycle {
        log(db, id, &format!("Cycle set to {}", extra::cycle_name(extra::cycle_choice(&it.cycle, now.date())).to_lowercase()), now);
    }
    undo_push(db, &format!("Edited: {}", it.title), &Undo { restore: vec![before], ..Undo::default() }, now)
}

/// Change a card's length (the calendar's bottom-edge drag). One free change with Undo.
pub fn resize_card(db: &Db, id: i64, dur_min: i64, now: NaiveDateTime) -> Res<i64> {
    let mut it = db.get(id).map_err(e)?.ok_or("That card no longer exists.")?;
    let before = it.clone();
    it.dur_min = dur_min.clamp(5, 24 * 60);
    db.put(&it).map_err(e)?;
    log(db, id, &format!("Length changed to {} min", it.dur_min), now);
    undo_push(db, &format!("Length: {}", it.title), &Undo { restore: vec![before], ..Undo::default() }, now)
}

fn day_has_fixed_event(items: &[Item], skip: i64, day: NaiveDate) -> bool {
    items.iter().any(|x| x.is_event() && !x.done && x.id != skip && x.at_local().map_or(false, |a| a.date() == day))
}

/// A manual move (drag or "Move to..."). Returns Applied for a single free move, else a Proposal.
pub fn submit_move(db: &Db, moves: &[(i64, NaiveDateTime)], now: NaiveDateTime) -> Res<Outcome> {
    let m: Vec<(i64, NaiveDateTime, bool)> = moves.iter().map(|(i, t)| (*i, *t, false)).collect();
    submit_move_ex(db, &m, now)
}

/// Same, with an all-day flag per move (a card dropped on a month day or the all-day row).
pub fn submit_move_ex(db: &Db, moves: &[(i64, NaiveDateTime, bool)], now: NaiveDateTime) -> Res<Outcome> {
    if moves.is_empty() {
        return Err("Nothing to move.".into());
    }
    let items = db.list(false).map_err(e)?;
    let mut mvs = Vec::new();
    for (id, to, allday) in moves {
        let it = items.iter().find(|x| x.id == *id).ok_or("That task no longer exists.")?;
        mvs.push(Mv {
            id: *id,
            title: it.title.clone(),
            from: it.at_local().filter(|_| it.has_time),
            to: *to,
            to_day_has_fixed_event: day_has_fixed_event(&items, *id, to.date()),
            allday: *allday,
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
/// Proposals the user reviews on the Daily planner page (suggested plans). Move, archive and delete questions are asked in a dialog at once, so they never count as waiting.
pub fn waiting_plans(db: &Db) -> Vec<(i64, Change)> {
    db.pending_proposals().unwrap_or_default().into_iter().filter(|(_, c)| matches!(c, Change::Plan { .. })).collect()
}

fn dispatch(db: &Db, change: Change, now: NaiveDateTime) -> Res<Outcome> {
    if needs_approval(&change) {
        // a dialog question is answered at once: a question nobody answered (page closed) is dropped, not kept waiting
        for (pid, c) in db.pending_proposals().map_err(e)? {
            if !matches!(c, Change::Plan { .. }) {
                db.set_proposal_status(pid, "cancelled").map_err(e)?;
            }
        }
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
        if m.allday {
            let day = m.to.date().and_time(NaiveTime::default());
            it.at = Some(to_utc(day));
            it.deadline = day.date().and_hms_opt(23, 59, 59).map(to_utc);
            it.has_time = false;
            it.remind_at = remind_at_for(Some(day), false, it.reminder_min);
        } else {
            it.at = Some(to_utc(m.to));
            if !auto && it.has_time {
                it.deadline = it.at; // a manual move of a timed item sets its new time as the deadline
            }
            it.has_time = true;
            it.remind_at = remind_at_for(Some(m.to), true, it.reminder_min);
        }
        it.fired = false;
        it.acked = false;
        db.put(&it).map_err(e)?;
        log(db, it.id, &due_note(it.at_local(), it.has_time), now);
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
        Change::MoveCards { moves } => {
            let m: Vec<(i64, i64, Option<usize>)> = moves.iter().map(|m| (m.id, m.to_list_id, m.index)).collect();
            place_cards(db, &m, now)
        }
        Change::ArchiveList { id, name, .. } => {
            let mut l = db.get_list(*id).map_err(e)?.ok_or("That list no longer exists.")?;
            let before = l.clone();
            l.archived = true;
            db.put_list(&l).map_err(e)?;
            undo_push(db, &format!("Archived list: {name}"), &Undo { restore_lists: vec![before], ..Undo::default() }, now)
        }
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
    for l in &u.restore_lists {
        db.put_list(l).map_err(e)?;
    }
    for id in &u.remove_items {
        db.delete(*id).map_err(e)?;
    }
    for id in &u.remove_lists {
        db.delete_list(*id).map_err(e)?;
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
                allday: false,
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

/// Example board so the first screen is never empty. Safe to press once.
pub fn load_sample(db: &Db, now: NaiveDateTime) -> Res<usize> {
    if db.list(true).map_err(e)?.iter().any(|x| x.sample) {
        return Err("The sample board is already loaded.".into());
    }
    let lists = db.lists(false).map_err(e)?;
    let todo = default_list(db)?;
    let inbox = db.inbox_id();
    let doing = lists.iter().filter(|l| !l.done_list && Some(l.id) != inbox).nth(1).map(|l| l.id).unwrap_or(todo);
    let done = lists.iter().find(|l| l.done_list).map(|l| l.id).unwrap_or(todo);
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
    let ck = |v: &[(&str, bool)]| v.iter().map(|(t, d)| Check { t: t.to_string(), d: *d }).collect::<Vec<_>>();
    // (card, list, labels, notes, checklist)
    let list: Vec<(NewItem, i64, &str, &str, Vec<Check>)> = vec![
        (mk("Pay electricity bill", false, Some(day(-1)), None, 30, 2, None), todo, "red", "Account number is on the paper bill.", vec![]),
        (mk("Send project report", false, Some(day(2)), None, 90, 2, None), todo, "purple,blue", "Send to the team before the meeting.", ck(&[("Write summary", true), ("Add charts", false), ("Email it", false)])),
        (mk("Buy groceries", false, Some(today), None, 45, 0, None), todo, "green", "", ck(&[("Milk", false), ("Bread", false), ("Apples", false)])),
        (mk("Call John", false, Some(day(1)), t(17, 0), 30, 1, None), todo, "yellow", "", vec![]),
        (mk("Dentist appointment", true, Some(day(1)), t(16, 30), 60, 1, None), todo, "orange", "A fixed event: the planner will not move it.", vec![]),
        (mk("Read 20 pages", false, None, None, 30, 0, None), todo, "", "", vec![]),
        (mk("Water the plants", false, Some(today), t(20, 0), 30, 1, Some(Repeat::Daily(1))), doing, "green", "", vec![]),
        (mk("Weekly review", false, Some(today + Duration::days((4 - today.weekday().num_days_from_monday() as i64 + 7) % 7)), t(16, 0), 30, 1, Some(Repeat::Weekly { every: 1, days: vec![4] })), doing, "blue", "", ck(&[("Check the board", true), ("Plan next week", false)])),
        (mk("Lunch with Maya", true, Some(today), t(13, 0), 60, 1, None), doing, "orange", "", vec![]),
        (mk("Book the dentist", false, None, None, 30, 1, None), done, "", "", vec![]),
    ];
    let n = list.len();
    for (x, lid, labels, notes, checks) in list {
        let mut it = create_in(db, &x, true, lid)?;
        it.labels = labels.to_string();
        it.notes = notes.to_string();
        it.checklist = checks;
        if it.remind_at.map_or(false, |r| r <= to_utc(now)) {
            it.fired = true; // no pop-up banner for sample cards that are already late
            it.acked = true;
        }
        db.put(&it).map_err(e)?;
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
    fn cancelled_or_forgotten_questions_leave_no_waiting_banner() {
        let db = Db::memory().unwrap();
        let ev = task(&db, "Dentist", Some(24), Some(10), true);
        let Outcome::Proposal(p) = submit_delete(&db, &[ev.id], now()).unwrap() else { panic!("asks first") };
        assert_eq!(db.pending_proposals().unwrap().len(), 1);
        cancel(&db, p).unwrap();
        assert!(db.pending_proposals().unwrap().is_empty(), "cancel discards it on the server");
        assert!(waiting_plans(&db).is_empty());
        // a question left unanswered (tab closed) never shows as waiting, and the next question drops it
        let Outcome::Proposal(q) = submit_delete(&db, &[ev.id], now()).unwrap() else { panic!("asks first") };
        assert!(waiting_plans(&db).is_empty());
        let Outcome::Proposal(_) = submit_delete(&db, &[ev.id], now()).unwrap() else { panic!("asks first") };
        assert_eq!(db.get_proposal(q).unwrap().unwrap().1, "cancelled");
        assert_eq!(db.pending_proposals().unwrap().len(), 1);
        // a suggested plan is the one thing that does wait for review
        task(&db, "Report", Some(25), None, false);
        make_plan(&db, now(), false).unwrap().unwrap();
        assert_eq!(waiting_plans(&db).len(), 1);
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

    fn list_id(db: &Db, name: &str) -> i64 {
        db.lists(false).unwrap().into_iter().find(|l| l.name == name).unwrap().id
    }
    fn titles(db: &Db, list: &str) -> Vec<String> {
        db.cards_in(list_id(db, list)).unwrap().into_iter().map(|c| c.title).collect()
    }
    fn plain(title: &str) -> NewItem {
        NewItem { title: title.into(), is_event: false, date: None, time: None, dur_min: 30, priority: 1, reminder_min: None, repeat: None, until: None }
    }

    #[test]
    fn a_new_board_has_three_lists_and_cards_land_in_to_do() {
        let db = Db::memory().unwrap();
        let names: Vec<String> = db.lists(false).unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["To do", "Doing", "Done"]);
        assert!(db.lists(false).unwrap()[2].done_list);
        create(&db, &plain("A"), false).unwrap();
        assert_eq!(titles(&db, "To do"), ["A"]);
    }

    #[test]
    fn card_order_drag_reorders_within_and_between_lists() {
        let db = Db::memory().unwrap();
        let ids: Vec<i64> = ["A", "B", "C"].iter().map(|t| create(&db, &plain(t), false).unwrap().id).collect();
        let (todo, doing) = (list_id(&db, "To do"), list_id(&db, "Doing"));
        place_cards(&db, &[(ids[2], todo, Some(0))], now()).unwrap();
        assert_eq!(titles(&db, "To do"), ["C", "A", "B"]);
        place_cards(&db, &[(ids[0], todo, Some(2))], now()).unwrap();
        assert_eq!(titles(&db, "To do"), ["C", "B", "A"]);
        place_cards(&db, &[(ids[1], doing, Some(0))], now()).unwrap();
        assert_eq!(titles(&db, "To do"), ["C", "A"]);
        assert_eq!(titles(&db, "Doing"), ["B"]);
        place_cards(&db, &[(ids[0], doing, Some(0))], now()).unwrap();
        assert_eq!(titles(&db, "Doing"), ["A", "B"]);
        place_cards(&db, &[(ids[2], doing, Some(99))], now()).unwrap(); // past the end = the end
        assert_eq!(titles(&db, "Doing"), ["A", "B", "C"]);
    }

    #[test]
    fn one_card_drag_applies_and_undo_restores_the_order() {
        let db = Db::memory().unwrap();
        let ids: Vec<i64> = ["A", "B"].iter().map(|t| create(&db, &plain(t), false).unwrap().id).collect();
        let doing = list_id(&db, "Doing");
        let Outcome::Applied(u) = submit_card_moves(&db, &[(ids[0], doing, None)], now()).unwrap() else { panic!("one drag is free") };
        assert_eq!(titles(&db, "Doing"), ["A"]);
        undo(&db, u).unwrap();
        assert_eq!(titles(&db, "To do"), ["A", "B"]);
        assert!(titles(&db, "Doing").is_empty());
    }

    #[test]
    fn moving_to_done_completes_and_moving_out_reopens() {
        let db = Db::memory().unwrap();
        let c = create(&db, &plain("Write"), false).unwrap();
        let (done, doing) = (list_id(&db, "Done"), list_id(&db, "Doing"));
        let Outcome::Applied(u) = submit_card_moves(&db, &[(c.id, done, None)], now()).unwrap() else { panic!() };
        assert!(db.get(c.id).unwrap().unwrap().done);
        assert_eq!(db.completions_between(0, i64::MAX).unwrap().len(), 1);
        undo(&db, u).unwrap();
        assert!(!db.get(c.id).unwrap().unwrap().done);
        assert!(db.completions_between(0, i64::MAX).unwrap().is_empty());
        complete(&db, c.id, now()).unwrap();
        assert_eq!(titles(&db, "Done"), ["Write"]);
        place_cards(&db, &[(c.id, doing, None)], now()).unwrap();
        assert!(!db.get(c.id).unwrap().unwrap().done);
        assert!(complete(&db, c.id, now()).is_ok());
        assert!(complete(&db, c.id, now()).is_err());
    }

    #[test]
    fn done_on_repeating_card_spawns_the_next_copy_in_to_do() {
        let db = Db::memory().unwrap();
        let n = NewItem { title: "Water".into(), is_event: false, date: Some(now().date()), time: NaiveTime::from_hms_opt(20, 0, 0), dur_min: 10, priority: 1, reminder_min: None, repeat: Some(Repeat::Daily(1)), until: None };
        let it = create_in(&db, &n, false, list_id(&db, "Doing")).unwrap();
        let mut chk = db.get(it.id).unwrap().unwrap();
        chk.checklist = vec![Check { t: "x".into(), d: true }];
        db.put(&chk).unwrap();
        let u = complete(&db, it.id, now()).unwrap();
        let old = db.get(it.id).unwrap().unwrap();
        assert!(old.done);
        assert_eq!(old.list_id, list_id(&db, "Done"));
        let todo = db.cards_in(list_id(&db, "To do")).unwrap();
        assert_eq!(todo.len(), 1);
        assert_eq!(todo[0].title, "Water");
        assert_eq!(todo[0].at_local(), Some(dt(24, 20, 0)));
        assert!(!todo[0].done && todo[0].repeat.is_some());
        assert!(todo[0].checklist.iter().all(|c| !c.d), "checklist starts fresh");
        // finishing the old card again later cannot spawn a second copy
        assert!(old.repeat.is_none());
        undo(&db, u).unwrap();
        assert!(db.cards_in(list_id(&db, "To do")).unwrap().is_empty());
        assert_eq!(db.completions_between(0, i64::MAX).unwrap().len(), 0);
    }

    #[test]
    fn series_with_until_does_not_spawn() {
        let db = Db::memory().unwrap();
        let n = NewItem { title: "Course".into(), is_event: false, date: Some(now().date()), time: None, dur_min: 30, priority: 1, reminder_min: None, repeat: Some(Repeat::Daily(1)), until: Some(now().date()) };
        let it = create(&db, &n, false).unwrap();
        complete(&db, it.id, now()).unwrap();
        assert!(db.get(it.id).unwrap().unwrap().done);
        assert!(titles(&db, "To do").is_empty());
    }

    #[test]
    fn bulk_card_moves_of_three_wait_for_approval() {
        let db = Db::memory().unwrap();
        let ids: Vec<i64> = ["A", "B", "C"].iter().map(|t| create(&db, &plain(t), false).unwrap().id).collect();
        let doing = list_id(&db, "Doing");
        let two: Vec<_> = ids[..2].iter().map(|i| (*i, doing, None)).collect();
        assert!(matches!(submit_card_moves(&db, &two, now()).unwrap(), Outcome::Applied(_)));
        let todo = list_id(&db, "To do");
        assert!(matches!(submit_move_all(&db, doing, todo, now()).unwrap(), Outcome::Applied(_)), "two cards are not gated");
        let three: Vec<_> = ids.iter().map(|i| (*i, doing, None)).collect();
        let Outcome::Proposal(p) = submit_card_moves(&db, &three, now()).unwrap() else { panic!("3 cards need approval") };
        assert_eq!(titles(&db, "To do").len() + titles(&db, "Doing").len(), 3);
        assert_eq!(titles(&db, "Doing").len(), 0, "nothing moved before Approve");
        approve(&db, p, now()).unwrap();
        assert_eq!(titles(&db, "Doing"), ["A", "B", "C"]);
    }

    #[test]
    fn archiving_a_list_with_cards_needs_approval_and_undo_brings_it_back() {
        let db = Db::memory().unwrap();
        let doing = list_id(&db, "Doing");
        // empty list: free
        let extra = {
            add_list(&db, "Ideas", now()).unwrap();
            list_id(&db, "Ideas")
        };
        assert!(matches!(submit_archive_list(&db, extra, now()).unwrap(), Outcome::Applied(_)));
        assert!(db.lists(false).unwrap().iter().all(|l| l.name != "Ideas"));
        // list with a card: asks first
        let c = create_in(&db, &plain("Keep me"), false, doing).unwrap();
        let Outcome::Proposal(p) = submit_archive_list(&db, doing, now()).unwrap() else { panic!("needs approval") };
        assert!(db.lists(false).unwrap().iter().any(|l| l.name == "Doing"), "still there before Approve");
        let u = approve(&db, p, now()).unwrap();
        assert!(db.lists(false).unwrap().iter().all(|l| l.name != "Doing"));
        assert!(db.list(true).unwrap().iter().all(|x| x.id != c.id), "cards of an archived list leave the calendar");
        undo(&db, u).unwrap();
        assert!(db.lists(false).unwrap().iter().any(|l| l.name == "Doing"));
        assert_eq!(titles(&db, "Doing"), ["Keep me"]);
    }

    #[test]
    fn the_last_open_list_and_the_done_list_cannot_be_archived() {
        let db = Db::memory().unwrap();
        assert!(submit_archive_list(&db, list_id(&db, "Done"), now()).is_err());
        let doing = list_id(&db, "Doing");
        assert!(submit_archive_list(&db, doing, now()).is_ok());
        assert!(submit_archive_list(&db, list_id(&db, "To do"), now()).is_err());
    }

    #[test]
    fn lists_can_be_added_renamed_and_reordered() {
        let db = Db::memory().unwrap();
        add_list(&db, "  Ideas ", now()).unwrap();
        assert!(add_list(&db, "   ", now()).is_err());
        let names = |db: &Db| db.lists(false).unwrap().into_iter().map(|l| l.name).collect::<Vec<_>>();
        assert_eq!(names(&db), ["To do", "Doing", "Done", "Ideas"]);
        let ideas = list_id(&db, "Ideas");
        let u = rename_list(&db, ideas, "Someday", now()).unwrap();
        assert_eq!(names(&db)[3], "Someday");
        undo(&db, u).unwrap();
        assert_eq!(names(&db)[3], "Ideas");
        let u = move_list(&db, ideas, 0, now()).unwrap();
        assert_eq!(names(&db), ["Ideas", "To do", "Doing", "Done"]);
        undo(&db, u).unwrap();
        assert_eq!(names(&db), ["To do", "Doing", "Done", "Ideas"]);
        move_list(&db, list_id(&db, "To do"), 99, now()).unwrap();
        assert_eq!(names(&db), ["Doing", "Done", "Ideas", "To do"]);
    }

    #[test]
    fn card_panel_save_checks_input_and_keeps_labels_in_the_palette() {
        let db = Db::memory().unwrap();
        let c = create(&db, &plain("A"), false).unwrap();
        let mut f = CardForm {
            title: " New title ".into(),
            notes: "n".into(),
            priority: 9,
            labels: vec!["red".into(), "evil".into(), "blue".into()],
            checklist: vec![Check { t: " one ".into(), d: true }, Check { t: "  ".into(), d: false }],
            date: Some(now().date()),
            time: NaiveTime::from_hms_opt(17, 0, 0),
            reminder_min: None,
            repeat: None,
            until: None,
            is_event: false,
            dur_min: 30,
        };
        update_card(&db, c.id, &f, now()).unwrap();
        let g = db.get(c.id).unwrap().unwrap();
        assert_eq!(g.title, "New title");
        assert_eq!(g.priority, 2);
        assert_eq!(g.labels, "blue,red".split(',').rev().collect::<Vec<_>>().join(","));
        assert_eq!(g.checklist, vec![Check { t: "one".into(), d: true }]);
        assert!(g.has_time && g.remind_at.is_some());
        f.title = "   ".into();
        assert!(update_card(&db, c.id, &f, now()).is_err());
        f.title = "ok".into();
        f.notes = "x".repeat(5001);
        assert!(update_card(&db, c.id, &f, now()).is_err());
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

#[cfg(test)]
mod linear_tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 24).unwrap().and_hms_opt(10, 0, 0).unwrap()
    }
    fn plain(title: &str) -> NewItem {
        NewItem { title: title.into(), is_event: false, date: None, time: None, dur_min: 30, priority: 1, reminder_min: None, repeat: None, until: None }
    }
    fn texts(db: &Db, id: i64) -> Vec<String> {
        db.activity_for(id, 50).unwrap().into_iter().map(|a| a.0).rev().collect()
    }
    fn form(c: &Item) -> CardForm {
        CardForm {
            title: c.title.clone(),
            notes: c.notes.clone(),
            priority: c.priority,
            labels: c.label_keys().iter().map(|s| s.to_string()).collect(),
            checklist: c.checklist.clone(),
            date: c.at_local().map(|a| a.date()),
            time: c.at_local().filter(|_| c.has_time).map(|a| a.time()),
            reminder_min: c.reminder_min,
            repeat: c.repeat.clone(),
            until: c.until,
            is_event: c.is_event(),
            dur_min: c.dur_min,
        }
    }

    #[test]
    fn activity_log_records_created_moved_completed_due_and_label() {
        let db = Db::memory().unwrap();
        let c = create(&db, &plain("Pay bill"), false).unwrap();
        assert_eq!(texts(&db, c.id), ["Created in To do"]);
        let lists = db.lists(false).unwrap();
        let (doing, done) = (lists[1].id, lists[2].id);
        place_cards(&db, &[(c.id, doing, None)], now()).unwrap();
        assert_eq!(texts(&db, c.id).last().unwrap(), "Moved from To do to Doing");
        let mut f = form(&db.get(c.id).unwrap().unwrap());
        f.date = Some(now().date() + Duration::days(1));
        f.labels = vec!["green".into()];
        update_card(&db, c.id, &f, now()).unwrap();
        let t = texts(&db, c.id);
        assert!(t.contains(&"Due changed to Wed 25 Nov".to_string()), "{t:?}");
        assert!(t.contains(&"Label added: Money".to_string()), "{t:?}");
        update_card(&db, c.id, &f, now()).unwrap(); // saving again changes nothing, so logs nothing
        assert_eq!(texts(&db, c.id).len(), t.len());
        complete(&db, c.id, now()).unwrap();
        let t = texts(&db, c.id);
        assert_eq!(&t[t.len() - 2..], ["Moved from Doing to Done", "Completed"]);
        place_cards(&db, &[(c.id, doing, None)], now()).unwrap();
        assert_eq!(texts(&db, c.id).last().unwrap(), "Reopened");
        let _ = done;
        // calendar drags log the new time; removing the date is logged too
        submit_move(&db, &[(c.id, now().date().and_hms_opt(15, 0, 0).unwrap())], now()).unwrap();
        assert_eq!(texts(&db, c.id).last().unwrap(), "Due changed to Tue 24 Nov, 3:00 PM");
        let mut f = form(&db.get(c.id).unwrap().unwrap());
        f.date = None;
        f.time = None;
        update_card(&db, c.id, &f, now()).unwrap();
        assert_eq!(texts(&db, c.id).last().unwrap(), "Due date removed");
    }

    #[test]
    fn card_lengths_default_to_30_and_resize_with_undo() {
        let db = Db::memory().unwrap();
        let c = create(&db, &plain("Write"), false).unwrap();
        assert_eq!(c.dur_min, 30);
        let u = resize_card(&db, c.id, 90, now()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().dur_min, 90);
        assert_eq!(texts(&db, c.id).last().unwrap(), "Length changed to 90 min");
        undo(&db, u).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().dur_min, 30);
        resize_card(&db, c.id, 1, now()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().dur_min, 5, "never shorter than 5 minutes");
        resize_card(&db, c.id, 99_999, now()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().dur_min, 1440);
    }

    #[test]
    fn weekly_cycle_is_saved_logged_and_undone() {
        let db = Db::memory().unwrap();
        let c = create(&db, &plain("Plan"), false).unwrap();
        let f = form(&c);
        let u = update_card_ext(&db, c.id, &f, Some("this"), now()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().cycle, "2026-11-23");
        assert_eq!(texts(&db, c.id).last().unwrap(), "Cycle set to this week");
        update_card_ext(&db, c.id, &f, Some("past"), now()).unwrap(); // "past" keeps what is stored
        assert_eq!(db.get(c.id).unwrap().unwrap().cycle, "2026-11-23");
        update_card_ext(&db, c.id, &f, Some("next"), now()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().cycle, "2026-11-30");
        update_card_ext(&db, c.id, &f, Some("none"), now()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().cycle, "");
        undo(&db, u).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().cycle, "");
    }

    #[test]
    fn inbox_is_added_once_at_the_left_and_quick_capture_lands_there() {
        let db = Db::memory().unwrap();
        db.ensure_inbox().unwrap();
        db.ensure_inbox().unwrap();
        let names: Vec<String> = db.lists(false).unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["Inbox", "To do", "Doing", "Done"]);
        let inbox = db.inbox_id().unwrap();
        assert_eq!(capture_list(&db).unwrap(), inbox);
        assert_ne!(default_list(&db).unwrap(), inbox, "the add boxes and repeat copies still use To do");
        let c = create_in(&db, &plain("Quick"), false, capture_list(&db).unwrap()).unwrap();
        assert_eq!(db.get(c.id).unwrap().unwrap().list_id, inbox);
        assert_eq!(texts(&db, c.id), ["Created in Inbox"]);
        // the sample board does not put cards in the Inbox
        load_sample(&db, now()).unwrap();
        assert!(db.cards_in(inbox).unwrap().len() == 1);
    }

    #[test]
    fn a_card_dropped_on_a_month_day_stays_all_day_and_big_changes_still_ask() {
        let db = Db::memory().unwrap();
        let c = create(&db, &plain("Loose"), false).unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap().and_hms_opt(0, 0, 0).unwrap();
        submit_move_ex(&db, &[(c.id, day, true)], now()).unwrap();
        let g = db.get(c.id).unwrap().unwrap();
        assert!(!g.has_time);
        assert_eq!(g.at_local(), Some(day));
        // three cards at once still wait for Approve
        let ids: Vec<i64> = (0..3).map(|i| create(&db, &plain(&format!("T{i}")), false).unwrap().id).collect();
        let mv: Vec<(i64, NaiveDateTime, bool)> = ids.iter().map(|i| (*i, day, false)).collect();
        assert!(matches!(submit_move_ex(&db, &mv, now()).unwrap(), Outcome::Proposal(_)));
        assert!(db.get(ids[0]).unwrap().unwrap().at.is_none(), "nothing written before Approve");
    }
}
