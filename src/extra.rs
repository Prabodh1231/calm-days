//! Small pure helpers for the Linear-style features: short ids, label suggestions, relative time,
//! weekly cycles, the Weekly Pulse and the calendar's side-by-side layout. No database here.
use crate::store::Item;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};

/// "CD-12": the short id shown on a card face, in the panel and in search.
pub fn short_id(id: i64) -> String {
    format!("CD-{id}")
}

/// What each colour label means in the UI. The colour key stays what is stored.
pub const LABEL_TITLES: [(&str, &str); 6] =
    [("green", "Money"), ("red", "Health"), ("blue", "People"), ("purple", "Work"), ("orange", "Home"), ("yellow", "Ideas")];

pub fn label_title(color: &str) -> &'static str {
    LABEL_TITLES.iter().find(|(c, _)| *c == color).map(|(_, t)| *t).unwrap_or("")
}

const WORDS: [(&str, &[&str]); 5] = [
    ("green", &["pay", "bill", "rent", "invoice", "tax", "bank", "budget", "salary", "refund", "insurance"]),
    ("red", &["gym", "run", "running", "workout", "doctor", "dentist", "yoga", "walk", "medicine", "exercise", "swim"]),
    ("blue", &["call", "email", "text", "message", "meet", "lunch", "dinner", "birthday", "visit", "reply"]),
    ("purple", &["report", "project", "meeting", "deadline", "client", "review", "presentation", "slides"]),
    ("orange", &["clean", "laundry", "groceries", "grocery", "repair", "plants", "cook", "shopping", "garden"]),
];

fn word_matches(word: &str, kw: &str) -> bool {
    word == kw || [("s"), ("es"), ("ed"), ("ing")].iter().any(|suf| word.strip_suffix(suf) == Some(kw))
}

/// Colour labels a card's words point to, e.g. "Pay the bill" -> ["green"]. Only a suggestion:
/// the user accepts it. Colours already on the card are left out.
pub fn suggest_labels(title: &str, notes: &str, have: &[String]) -> Vec<&'static str> {
    let text = format!("{title} {notes}").to_lowercase();
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    WORDS
        .iter()
        .filter(|(color, kws)| !have.iter().any(|h| h == color) && words.iter().any(|w| kws.iter().any(|k| word_matches(w, k))))
        .map(|(c, _)| *c)
        .collect()
}

/// "just now", "2 min ago", "3 h ago", "4 d ago".
pub fn ago(now_utc: i64, then_utc: i64) -> String {
    let s = (now_utc - then_utc).max(0);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// The value stored for a cycle choice: the Monday of that week. "none" gives "".
pub fn cycle_value(choice: &str, today: NaiveDate) -> Option<String> {
    let w = week_start(today);
    match choice {
        "this" => Some(w.format("%Y-%m-%d").to_string()),
        "next" => Some((w + Duration::days(7)).format("%Y-%m-%d").to_string()),
        "" | "none" => Some(String::new()),
        _ => None, // "past" or unknown: keep what is stored
    }
}

/// How a stored cycle reads today: "this", "next", "past" (an earlier week), "later" or "".
pub fn cycle_choice(stored: &str, today: NaiveDate) -> &'static str {
    let Ok(d) = NaiveDate::parse_from_str(stored, "%Y-%m-%d") else { return "" };
    match (d - week_start(today)).num_days() {
        0 => "this",
        7 => "next",
        n if n < 0 => "past",
        _ => "later",
    }
}

pub fn cycle_name(choice: &str) -> &'static str {
    match choice {
        "this" => "This week",
        "next" => "Next week",
        "past" => "Earlier week",
        "later" => "Later week",
        _ => "No cycle",
    }
}

/// (done, total) of the cards in this week's cycle.
pub fn cycle_stats(items: &[Item], today: NaiveDate) -> (usize, usize) {
    let mine: Vec<&Item> = items.iter().filter(|i| !i.archived && cycle_choice(&i.cycle, today) == "this").collect();
    (mine.iter().filter(|i| i.done).count(), mine.len())
}

#[derive(Debug, PartialEq)]
pub struct Pulse {
    pub done_week: usize,
    pub slipped: usize,
    pub at_risk: usize,
    pub cycle_done: usize,
    pub cycle_total: usize,
}

/// The numbers for the Weekly Pulse. Slipped = open and overdue. At risk = open, due today or in the next
/// 2 days, and not started: it sits in one of `not_started` lists, or (when none are given) has no checklist tick.
pub fn pulse(done_week: usize, items: &[Item], not_started: &[i64], now: NaiveDateTime) -> Pulse {
    let today = now.date();
    let open: Vec<&Item> = items.iter().filter(|i| !i.done && !i.archived && !i.is_event()).collect();
    let slipped = open.iter().filter(|i| i.is_overdue(now)).count();
    let at_risk = open
        .iter()
        .filter(|i| !i.is_overdue(now))
        .filter(|i| i.at_local().map_or(false, |a| (0..=2).contains(&(a.date() - today).num_days())))
        .filter(|i| if not_started.is_empty() { !i.checklist.iter().any(|c| c.d) } else { not_started.contains(&i.list_id) })
        .count();
    let (cycle_done, cycle_total) = cycle_stats(items, today);
    Pulse { done_week, slipped, at_risk, cycle_done, cycle_total }
}

impl Pulse {
    pub fn lines(&self) -> Vec<String> {
        let mut v = vec![format!("Done this week: {}.", self.done_week)];
        v.push(if self.slipped == 0 { "Slipped: nothing is overdue.".into() } else { format!("Slipped: {} overdue.", self.slipped) });
        v.push(if self.at_risk == 0 {
            "At risk: nothing due soon is waiting.".into()
        } else {
            format!("At risk: {} due in the next 2 days and not started.", self.at_risk)
        });
        if self.cycle_total > 0 {
            v.push(format!("This week's cycle: {} of {} done.", self.cycle_done, self.cycle_total));
        }
        v
    }
}

/// Side-by-side columns for overlapping blocks. Input: (id, start minute, end minute).
/// Output: (id, column, columns in its cluster).
pub fn layout_columns(ev: &[(i64, i64, i64)]) -> Vec<(i64, usize, usize)> {
    let mut sorted: Vec<(i64, i64, i64)> = ev.to_vec();
    sorted.sort_by_key(|e| (e.1, e.2, e.0));
    let mut out = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let mut cluster_end = sorted[i].2;
        let mut j = i;
        let mut col_ends: Vec<i64> = Vec::new();
        let mut placed: Vec<(i64, usize)> = Vec::new();
        while j < sorted.len() && sorted[j].1 < cluster_end.max(sorted[i].1 + 1) {
            let (id, s, e) = sorted[j];
            let col = col_ends.iter().position(|end| *end <= s).unwrap_or_else(|| {
                col_ends.push(0);
                col_ends.len() - 1
            });
            col_ends[col] = e.max(s + 1);
            cluster_end = cluster_end.max(e);
            placed.push((id, col));
            j += 1;
        }
        let n = col_ends.len();
        out.extend(placed.into_iter().map(|(id, c)| (id, c, n)));
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Check;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }
    fn card(id: i64, due: Option<u32>, done: bool, list: i64) -> Item {
        Item {
            id,
            title: format!("c{id}"),
            kind: "task".into(),
            at: due.map(|x| crate::store::to_utc(d(x).and_hms_opt(0, 0, 0).unwrap())),
            has_time: false,
            dur_min: 30,
            deadline: None,
            priority: 1,
            reminder_min: None,
            remind_at: None,
            fired: false,
            acked: false,
            repeat: None,
            until: None,
            done,
            sample: false,
            list_id: list,
            pos: 0,
            notes: String::new(),
            labels: String::new(),
            checklist: vec![],
            archived: false,
            cycle: String::new(),
        }
    }

    #[test]
    fn short_ids_read_cd_number() {
        assert_eq!(short_id(12), "CD-12");
    }

    #[test]
    fn label_suggestions_follow_words_and_skip_chosen() {
        assert_eq!(suggest_labels("Pay the electricity bill", "", &[]), vec!["green"]);
        assert_eq!(suggest_labels("Go for a run", "", &[]), vec!["red"]);
        assert_eq!(suggest_labels("Gym at 6", "", &[]), vec!["red"]);
        assert_eq!(suggest_labels("Call John and email Sara", "", &[]), vec!["blue"]);
        assert_eq!(suggest_labels("Pay rent, call landlord", "", &[]), vec!["green", "blue"]);
        assert_eq!(suggest_labels("Pay bills", "", &["green".to_string()]), Vec::<&str>::new());
        assert!(suggest_labels("Read 20 pages", "", &[]).is_empty());
        assert!(suggest_labels("Payroll runway", "", &[]).is_empty(), "whole words only");
        assert_eq!(label_title("green"), "Money");
    }

    #[test]
    fn relative_time_reads_plainly() {
        assert_eq!(ago(1000, 1000), "just now");
        assert_eq!(ago(1120, 1000), "2 min ago");
        assert_eq!(ago(1000 + 3 * 3600, 1000), "3 h ago");
        assert_eq!(ago(1000 + 2 * 86400, 1000), "2 d ago");
    }

    #[test]
    fn cycles_are_weeks_starting_monday() {
        let tue = d(24);
        assert_eq!(cycle_value("this", tue).unwrap(), "2026-11-23");
        assert_eq!(cycle_value("next", tue).unwrap(), "2026-11-30");
        assert_eq!(cycle_value("none", tue).unwrap(), "");
        assert!(cycle_value("past", tue).is_none());
        assert_eq!(cycle_choice("2026-11-23", d(29)), "this", "Sunday is still this week");
        assert_eq!(cycle_choice("2026-11-23", d(30)), "past", "Monday starts a new week");
        assert_eq!(cycle_choice("2026-11-30", tue), "next");
        assert_eq!(cycle_choice("", tue), "");
        let mut a = card(1, None, true, 1);
        let mut b = card(2, None, false, 1);
        let mut c = card(3, None, false, 1);
        a.cycle = "2026-11-23".into();
        b.cycle = "2026-11-23".into();
        c.cycle = "2026-11-30".into();
        assert_eq!(cycle_stats(&[a, b, c, card(4, None, true, 1)], tue), (1, 2));
    }

    #[test]
    fn pulse_counts_done_slipped_and_at_risk() {
        let now = d(24).and_hms_opt(10, 0, 0).unwrap();
        let mut started = card(3, Some(25), false, 2);
        started.checklist = vec![Check { t: "x".into(), d: true }];
        let items = vec![
            card(1, Some(22), false, 1), // overdue -> slipped
            card(2, Some(25), false, 1), // due tomorrow, in To do -> at risk
            started,                     // due tomorrow, in Doing -> not at risk
            card(4, Some(30), false, 1), // later
            card(5, Some(22), true, 3),  // done
            card(6, None, false, 1),
        ];
        let p = pulse(4, &items, &[1], now);
        assert_eq!((p.done_week, p.slipped, p.at_risk), (4, 1, 1));
        let p2 = pulse(0, &items, &[], now); // no lists known: a ticked checklist means started
        assert_eq!((p2.slipped, p2.at_risk), (1, 1));
        let lines = p.lines();
        assert_eq!(lines[0], "Done this week: 4.");
        assert_eq!(lines[1], "Slipped: 1 overdue.");
        assert_eq!(lines[2], "At risk: 1 due in the next 2 days and not started.");
        assert_eq!(lines.len(), 3, "no cycle line without cycle cards");
    }

    #[test]
    fn overlapping_blocks_sit_side_by_side() {
        let l = layout_columns(&[(1, 540, 600), (2, 560, 620), (3, 700, 730), (4, 590, 650)]);
        let get = |id| l.iter().find(|x| x.0 == id).copied().unwrap();
        assert_eq!(get(1), (1, 0, 3));
        assert_eq!(get(2), (2, 1, 3));
        assert_eq!(get(4), (4, 2, 3));
        assert_eq!(get(3), (3, 0, 1), "a lone block uses the full width");
        // back-to-back blocks do not overlap
        let l = layout_columns(&[(1, 540, 570), (2, 570, 600)]);
        assert!(l.iter().all(|x| x.1 == 0 && x.2 == 1));
    }
}
