//! "Smart text reading": a hand-written rule reader for plain sentences. It runs offline.
//! No regex, no AI. It never panics; unknown words stay in the title.
//!
//! Rules worth knowing (also shown in the README and the UI):
//! * Numeric dates are day-first: 12/11 = 12 November.
//! * A bare time with no am/pm: 1-6 means PM, 7-11 means AM ("at 5" = 5 PM, "at 9" = 9 AM).
//! * A bare time that has already passed today moves to tomorrow. "today at 9 AM" stays today.
//! * "midnight" = the start of the next day. "noon" = 12:00.
//! * "Friday" = the coming Friday (today if it is Friday). "next Friday" = the coming Friday when it is 3 or more days away,
//!   otherwise the one after (Tue 24 Nov: next Friday = 27 Nov. Fri 27 Nov: next Friday = 4 Dec).
//! * "31st" in a short month = the last day of that month. "Feb 29" waits for the next leap year.
use crate::repeat::{self, Repeat};
use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime};

pub const MAX_INPUT_CHARS: usize = 500;
pub const MAX_TITLE_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    /// 0 low, 1 medium, 2 high
    pub priority: i64,
    /// None = default, Some(-1) = no reminder, Some(n) = n minutes before
    pub reminder_min: Option<i64>,
    pub repeat: Option<Repeat>,
    pub until: Option<NaiveDate>,
    pub dur_min: i64,
    pub is_event: bool,
    pub notes: Vec<String>,
}

impl Parsed {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "title": self.title,
            "date": self.date.map(|d| d.format("%Y-%m-%d").to_string()),
            "time": self.time.map(|t| t.format("%H:%M").to_string()),
            "priority": self.priority,
            "reminder_min": self.reminder_min,
            "repeat": self.repeat.as_ref().map(repeat::describe).unwrap_or_default(),
            "until": self.until.map(|d| d.format("%Y-%m-%d").to_string()),
            "dur_min": self.dur_min,
            "kind": if self.is_event { "event" } else { "task" },
            "notes": self.notes,
            "reminder_text": self.reminder_text(),
        })
    }

    /// Plain words for the reminder chip. None when there is no date, so nothing can fire.
    pub fn reminder_text(&self) -> Option<String> {
        self.date?;
        Some(match (self.reminder_min, self.time.is_some()) {
            (Some(-1), _) => "No reminder".into(),
            (None, true) => "Reminder: 10 min before".into(),
            (None, false) => "Reminder: 9:00 AM that day".into(),
            (Some(n), true) => format!("Reminder: {n} min before"),
            (Some(n), false) => format!("Reminder: {n} min before 9:00 AM"),
        })
    }
}

struct Tok {
    orig: String,
    low: String,
    used: bool,
    prot: bool,
}

struct P {
    t: Vec<Tok>,
    today: NaiveDate,
}

struct TimeHit {
    time: NaiveTime,
    n: usize,
    midnight: bool,
    note: Option<String>,
}

struct DateHit {
    date: NaiveDate,
    n: usize,
    part: Option<NaiveTime>,
}

fn norm(w: &str) -> String {
    let l = w.to_lowercase();
    if l.chars().all(|c| c == '!') {
        return l;
    }
    let mut s = l
        .trim_matches(|c: char| matches!(c, '"' | '“' | '”' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':' | '!' | '?'))
        .to_string();
    while s.ends_with('.') {
        s.pop();
    }
    let nd = s.replace('.', "");
    if nd.len() >= 2 && (nd.ends_with("am") || nd.ends_with("pm")) {
        let body = &nd[..nd.len() - 2];
        if body.chars().all(|c| c.is_ascii_digit() || c == ':') {
            s = nd.clone();
        }
    }
    s
}

fn num(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 6 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// "5th" -> 5. Needs the suffix.
fn ordinal(s: &str) -> Option<u32> {
    for suf in ["st", "nd", "rd", "th"] {
        if let Some(b) = s.strip_suffix(suf) {
            return num(b);
        }
    }
    None
}

fn count(s: &str) -> Option<u32> {
    match s {
        "a" | "an" => Some(1),
        _ => num(s),
    }
}

/// m minutes, h hours, d days, w weeks, M months
fn unit(s: &str) -> Option<char> {
    Some(match s {
        "minute" | "minutes" | "min" | "mins" => 'm',
        "hour" | "hours" | "hr" | "hrs" => 'h',
        "day" | "days" => 'd',
        "week" | "weeks" | "wk" | "wks" => 'w',
        "month" | "months" => 'M',
        _ => return None,
    })
}

fn weekday(s: &str) -> Option<u8> {
    let s = if s.len() > 3 && s.ends_with('s') && s != "thurs" && s != "tues" && s != "weds" { &s[..s.len() - 1] } else { s };
    Some(match s {
        "monday" | "mon" => 0,
        "tuesday" | "tue" | "tues" => 1,
        "wednesday" | "wed" | "weds" => 2,
        "thursday" | "thu" | "thur" | "thurs" => 3,
        "friday" | "fri" => 4,
        "saturday" | "sat" => 5,
        "sunday" | "sun" => 6,
        _ => return None,
    })
}

fn month_num(s: &str) -> Option<u32> {
    Some(match s {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sep" | "sept" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    })
}

fn add_days(d: NaiveDate, n: i64) -> Option<NaiveDate> {
    d.checked_add_signed(Duration::try_days(n)?)
}

fn hm(h: u32, m: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(h, m, 0)
}

impl P {
    fn new(text: &str, now: NaiveDateTime) -> P {
        let text: String = text.chars().take(MAX_INPUT_CHARS).collect::<String>().replace('@', " at ");
        let mut t = Vec::new();
        let mut in_q = false;
        for w in text.split_whitespace() {
            let opens = w.starts_with('"') || w.starts_with('“');
            let closes = w.chars().count() > 1 && (w.ends_with('"') || w.ends_with('”'));
            let mut prot = in_q;
            if !in_q && opens {
                prot = true;
                in_q = !closes;
            } else if in_q && (w.ends_with('"') || w.ends_with('”')) {
                in_q = false;
            }
            t.push(Tok { orig: w.to_string(), low: norm(w), used: false, prot });
        }
        P { t, today: now.date() }
    }

    fn free(&self, i: usize) -> bool {
        self.t.get(i).map_or(false, |k| !k.used && !k.prot)
    }
    fn w(&self, i: usize) -> &str {
        if self.free(i) {
            &self.t[i].low
        } else {
            ""
        }
    }
    fn prev_w(&self, i: usize) -> &str {
        match i.checked_sub(1).and_then(|p| self.t.get(p)) {
            Some(k) if !k.prot => &k.low,
            _ => "",
        }
    }
    fn mark(&mut self, i: usize, n: usize) {
        for k in self.t.iter_mut().skip(i).take(n) {
            k.used = true;
        }
    }

    // ---------- time ----------
    fn time_at(&self, i: usize) -> Option<TimeHit> {
        let s = self.w(i);
        if s.is_empty() {
            return None;
        }
        let hit = |h, m, midnight| Some(TimeHit { time: hm(h, m)?, n: 1, midnight, note: None });
        match s {
            "noon" => return hit(12, 0, false),
            "midnight" => return hit(0, 0, true),
            _ => {}
        }
        let prev = self.prev_w(i);
        let (body, mut pm) = if let Some(b) = s.strip_suffix("pm") {
            (b, Some(true))
        } else if let Some(b) = s.strip_suffix("am") {
            (b, Some(false))
        } else {
            (s, None)
        };
        if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ':') || body.matches(':').count() > 1 {
            return None;
        }
        let colon = body.contains(':');
        let mut n = 1;
        if pm.is_none() {
            match self.w(i + 1) {
                "pm" => {
                    pm = Some(true);
                    n = 2
                }
                "am" => {
                    pm = Some(false);
                    n = 2
                }
                _ => {}
            }
        }
        let (hs, ms) = body.split_once(':').unwrap_or((body, "0"));
        if hs.is_empty() || hs.len() > 2 || ms.is_empty() || ms.len() > 2 {
            return None;
        }
        let h: u32 = hs.parse().ok()?;
        let m: u32 = ms.parse().ok()?;
        if m > 59 {
            return None;
        }
        let at_like = prev == "at" || prev == "by";
        if pm.is_none() && !colon && !at_like {
            return None;
        }
        if !at_like && matches!(prev, "the" | "a" | "an" | "our" | "my" | "his" | "her" | "their" | "your") {
            return None; // "about the 5 PM meeting"
        }
        let mut note = None;
        let hh = match pm {
            Some(p) => {
                if !(1..=12).contains(&h) {
                    return None;
                }
                h % 12 + if p { 12 } else { 0 }
            }
            None => {
                if h > 23 {
                    return None;
                }
                if h == 0 || h >= 12 || (hs.len() == 2 && hs.starts_with('0')) {
                    h
                } else if h <= 6 {
                    note = Some(format!("Read the time as {h}:{m:02} PM. Add am or pm to be exact."));
                    h + 12
                } else {
                    note = Some(format!("Read the time as {h}:{m:02} AM. Add am or pm to be exact."));
                    h
                }
            }
        };
        Some(TimeHit { time: hm(hh, m)?, n, midnight: false, note })
    }

    // ---------- date ----------
    fn upcoming_weekday(&self, wd: u8) -> Option<NaiveDate> {
        let cur = self.today.weekday().num_days_from_monday() as i64;
        add_days(self.today, (wd as i64 - cur + 7) % 7)
    }
    fn next_week_weekday(&self, wd: u8) -> Option<NaiveDate> {
        let cur = self.today.weekday().num_days_from_monday() as i64;
        let ahead = (wd as i64 - cur + 7) % 7;
        add_days(self.today, if ahead >= 3 { ahead } else { ahead + 7 })
    }
    /// Next month/day on or after today (looks ahead 8 years so Feb 29 works).
    fn next_md(&self, m: u32, d: u32) -> Option<NaiveDate> {
        (self.today.year()..=self.today.year() + 8)
            .filter_map(|y| NaiveDate::from_ymd_opt(y, m, d))
            .find(|dt| *dt >= self.today)
    }

    fn date_at(&self, i: usize) -> Option<DateHit> {
        let s = self.w(i);
        if s.is_empty() {
            return None;
        }
        let mk = |date: Option<NaiveDate>, n: usize| date.map(|date| DateHit { date, n, part: None });
        let day_part = |base: Option<DateHit>| -> Option<DateHit> {
            let mut h = base?;
            let p = match self.w(i + h.n) {
                "morning" => hm(9, 0),
                "afternoon" => hm(15, 0),
                "evening" => hm(19, 0),
                "night" => hm(21, 0),
                _ => None,
            };
            if p.is_some() {
                h.n += 1;
                h.part = p;
            }
            Some(h)
        };
        match s {
            "today" => return day_part(mk(Some(self.today), 1)),
            "tomorrow" | "tmrw" | "tmr" => return day_part(mk(add_days(self.today, 1), 1)),
            "tonight" => return Some(DateHit { date: self.today, n: 1, part: hm(20, 0) }),
            "day" if self.w(i + 1) == "after" && self.w(i + 2) == "tomorrow" => return mk(add_days(self.today, 2), 3),
            "end" if self.w(i + 1) == "of" => {
                let n = match (self.w(i + 2), self.w(i + 3)) {
                    ("month", _) => 3,
                    ("the", "month") => 4,
                    _ => 0,
                };
                if n > 0 {
                    return mk(repeat::month_day(self.today.year(), self.today.month(), 31), n);
                }
            }
            "next" => match self.w(i + 1) {
                "week" => return mk(add_days(self.today, 7 - self.today.weekday().num_days_from_monday() as i64), 2),
                "month" => return mk(self.today.checked_add_months(Months::new(1)), 2),
                x => {
                    if let Some(wd) = weekday(x) {
                        return mk(self.next_week_weekday(wd), 2);
                    }
                }
            },
            "this" => {
                if let Some(wd) = weekday(self.w(i + 1)) {
                    return mk(self.upcoming_weekday(wd), 2);
                }
            }
            _ => {}
        }
        if let Some(wd) = weekday(s) {
            let short = !s.ends_with("day") && !s.ends_with("days");
            if !short || matches!(self.prev_w(i), "on" | "this" | "next" | "every" | "by" | "until" | "till" | "before") {
                return mk(self.upcoming_weekday(wd), 1);
            }
            return None;
        }
        // "Jan 5", "january 5th"
        if let Some(m) = month_num(s) {
            let nx = self.w(i + 1);
            let d = ordinal(nx).or_else(|| num(nx))?;
            if !(1..=31).contains(&d) || matches!(self.w(i + 2), "am" | "pm") {
                return None;
            }
            return mk(self.next_md(m, d), 2);
        }
        // "5 Jan", "5th january"
        if let Some(d) = ordinal(s).or_else(|| num(s).filter(|n| *n <= 31)) {
            if let Some(m) = month_num(self.w(i + 1)) {
                if (1..=31).contains(&d) {
                    return mk(self.next_md(m, d), 2);
                }
            }
        }
        // "5th" alone
        if let Some(d) = ordinal(s) {
            if (1..=31).contains(&d) {
                let (y, m) = (self.today.year(), self.today.month());
                let this = repeat::month_day(y, m, d)?;
                if this >= self.today {
                    return mk(Some(this), 1);
                }
                let nm = NaiveDate::from_ymd_opt(y, m, 1)?.checked_add_months(Months::new(1))?;
                return mk(repeat::month_day(nm.year(), nm.month(), d), 1);
            }
        }
        // 12/11 or 12/11/2026 (day first)
        if s.contains('/') {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 2 || parts.len() == 3 {
                let d = num(parts[0])?;
                let m = num(parts[1])?;
                if !(1..=31).contains(&d) || !(1..=12).contains(&m) {
                    return None;
                }
                if parts.len() == 3 {
                    let mut y = num(parts[2])? as i32;
                    if parts[2].len() == 2 {
                        y += 2000;
                    }
                    return mk(NaiveDate::from_ymd_opt(y, m, d), 1);
                }
                return mk(self.next_md(m, d), 1);
            }
        }
        // 2026-12-20
        if s.len() == 10 && s.matches('-').count() == 2 {
            return mk(NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(), 1);
        }
        None
    }

    // ---------- repeat ----------
    fn weekday_list(&self, mut k: usize) -> (Vec<u8>, usize) {
        let start = k;
        let mut days = Vec::new();
        while let Some(d) = weekday(self.w(k)) {
            if !days.contains(&d) {
                days.push(d);
            }
            k += 1;
            if self.w(k) == "and" && weekday(self.w(k + 1)).is_some() {
                k += 1;
            }
        }
        days.sort_unstable();
        (days, k - start)
    }

    /// "on mon and wed" after "every 2 weeks"
    fn weekly_tail(&self, k: usize) -> (Vec<u8>, usize) {
        if self.w(k) == "on" {
            let (d, n) = self.weekday_list(k + 1);
            if n > 0 {
                return (d, n + 1);
            }
        }
        (vec![], 0)
    }

    /// "on the 15th" after "monthly"
    fn monthly_tail(&self, k: usize) -> (u32, usize) {
        if self.w(k) == "on" {
            let (j, extra) = if self.w(k + 1) == "the" { (k + 2, 2) } else { (k + 1, 1) };
            if let Some(d) = ordinal(self.w(j)).or_else(|| num(self.w(j))) {
                if (1..=31).contains(&d) {
                    return (d, extra + 1);
                }
            }
        }
        (0, 0)
    }

    fn repeat_at(&self, i: usize) -> Option<(Repeat, usize)> {
        match self.w(i) {
            "daily" | "everyday" => return Some((Repeat::Daily(1), 1)),
            "weekly" => return Some((Repeat::Weekly { every: 1, days: vec![] }, 1)),
            "weekdays" => return Some((Repeat::Weekdays, 1)),
            "monthly" => {
                let (d, n) = self.monthly_tail(i + 1);
                return Some((Repeat::Monthly(d), 1 + n));
            }
            "every" | "each" => {}
            _ => return None,
        }
        let j = i + 1;
        let t = self.w(j);
        match t {
            "day" => return Some((Repeat::Daily(1), 2)),
            "weekday" | "weekdays" => return Some((Repeat::Weekdays, 2)),
            "week" => {
                let (days, n) = self.weekly_tail(j + 1);
                return Some((Repeat::Weekly { every: 1, days }, 2 + n));
            }
            "month" => {
                let (d, n) = self.monthly_tail(j + 1);
                return Some((Repeat::Monthly(d), 2 + n));
            }
            _ => {}
        }
        let (n_every, k) = if t == "other" { (Some(2), j + 1) } else { (num(t).filter(|n| (1..=365).contains(n)), j + 1) };
        if let Some(n) = n_every {
            let k2 = if t == "other" { k } else { k };
            return match unit(self.w(k2)) {
                Some('d') => Some((Repeat::Daily(n), k2 + 1 - i)),
                Some('w') => {
                    let (days, extra) = self.weekly_tail(k2 + 1);
                    Some((Repeat::Weekly { every: n, days }, k2 + 1 - i + extra))
                }
                Some('M') => Some((Repeat::Monthly(0), k2 + 1 - i)),
                _ => None,
            };
        }
        let (days, n) = self.weekday_list(j);
        if n > 0 {
            return Some((Repeat::Weekly { every: 1, days }, 1 + n));
        }
        None
    }

    // ---------- priority ----------
    fn priority_pass(&mut self) -> Option<i64> {
        let last = (0..self.t.len()).rev().find(|i| self.free(*i));
        let first = (0..self.t.len()).find(|i| self.free(*i));
        for i in 0..self.t.len() {
            let s = self.w(i).to_string();
            let lvl = |x: &str| match x {
                "high" => Some(2),
                "low" => Some(0),
                "medium" | "normal" => Some(1),
                _ => None,
            };
            match s.as_str() {
                "urgent" | "asap" | "critical" | "!!" | "!!!" => {
                    self.mark(i, 1);
                    return Some(2);
                }
                "high-priority" | "low-priority" => {
                    self.mark(i, 1);
                    return Some(if s.starts_with("high") { 2 } else { 0 });
                }
                "priority" => {
                    if let Some(l) = lvl(self.w(i + 1)) {
                        self.mark(i, 2);
                        return Some(l);
                    }
                }
                x => {
                    if let Some(l) = lvl(x) {
                        if matches!(self.w(i + 1), "priority" | "prio") {
                            self.mark(i, 2);
                            return Some(l);
                        }
                        if l != 1 && (Some(i) == first || Some(i) == last) && self.t.len() > 1 {
                            self.mark(i, 1);
                            return Some(l);
                        }
                    }
                }
            }
        }
        None
    }
}

const PREPS: [&str; 10] = ["at", "on", "by", "for", "in", "due", "from", "until", "till", "before"];

fn is_event_word(title_low: &str) -> bool {
    ["meeting", "appointment", "interview", "dentist", "class", "lunch", "dinner", "standup", "event"]
        .iter()
        .any(|k| title_low.contains(k))
}

pub fn parse_repeat_phrase(text: &str, now: NaiveDateTime) -> Option<Repeat> {
    let p = P::new(text, now);
    p.repeat_at(0).map(|(r, _)| r)
}

/// Read one sentence. `now` is local time.
pub fn parse(text: &str, now: NaiveDateTime) -> Parsed {
    let mut p = P::new(text, now);
    let mut notes: Vec<String> = Vec::new();
    let today = p.today;
    let mut reminder_min: Option<i64> = None;
    let mut reminder_time: Option<NaiveTime> = None;
    let mut dur_min: Option<i64> = None;

    // reminders: "remind me 30 minutes before", "remind me at 4"
    for i in 0..p.t.len() {
        if p.w(i) == "remind" && p.w(i + 1) == "me" {
            if let (Some(n), Some(u)) = (count(p.w(i + 2)), unit(p.w(i + 3))) {
                if p.w(i + 4) == "before" {
                    let mins = match u {
                        'm' => 1,
                        'h' => 60,
                        'd' => 1440,
                        'w' => 10080,
                        _ => 0,
                    } * n.min(10_000) as i64;
                    if mins > 0 {
                        reminder_min = Some(mins);
                        p.mark(i, 5);
                        break;
                    }
                }
            }
            if p.w(i + 2) == "at" {
                if let Some(h) = p.time_at(i + 3) {
                    reminder_time = Some(h.time);
                    if let Some(nt) = h.note {
                        notes.push(nt);
                    }
                    p.mark(i, 3 + h.n);
                    break;
                }
            }
        }
    }

    // duration: "for 2 hours" / "for 45 minutes"
    for i in 0..p.t.len() {
        if p.w(i) == "for" {
            if let (Some(n), Some(u)) = (count(p.w(i + 1)), unit(p.w(i + 2))) {
                if (u == 'h' || u == 'm') && n > 0 {
                    let m = if u == 'h' { n as i64 * 60 } else { n as i64 };
                    if m <= 24 * 60 {
                        dur_min = Some(m);
                        p.mark(i, 3);
                        break;
                    }
                }
            }
        }
    }

    // repeat
    let mut repeat: Option<Repeat> = None;
    for i in 0..p.t.len() {
        if let Some((r, n)) = p.repeat_at(i) {
            repeat = Some(r);
            p.mark(i, n);
            break;
        }
    }
    let mut until: Option<NaiveDate> = None;
    let mut span: Option<(u32, char)> = None;
    if repeat.is_some() {
        for i in 0..p.t.len() {
            if matches!(p.w(i), "until" | "till" | "through") {
                if let Some(h) = p.date_at(i + 1) {
                    until = Some(h.date);
                    p.mark(i, 1 + h.n);
                    break;
                }
            }
        }
        for i in 0..p.t.len() {
            if p.w(i) == "for" {
                if let (Some(n), Some(u)) = (count(p.w(i + 1)), unit(p.w(i + 2))) {
                    if matches!(u, 'd' | 'w' | 'M') && (1..=520).contains(&n) {
                        span = Some((n, u));
                        p.mark(i, 3);
                        break;
                    }
                }
            }
        }
    }

    let priority = p.priority_pass().unwrap_or(1);

    // date and time
    let mut date: Option<NaiveDate> = None;
    let mut time: Option<NaiveTime> = None;
    let mut midnight = false;
    let mut explicit = false;
    for i in 0..p.t.len() {
        if p.w(i) == "in" {
            if let (Some(n), Some(u)) = (count(p.w(i + 1)), unit(p.w(i + 2))) {
                let n = n as i64;
                let rel: Option<(NaiveDate, Option<NaiveTime>)> = match u {
                    'm' | 'h' if n <= 100_000 => {
                        let dt = Duration::try_minutes(if u == 'h' { n * 60 } else { n }).and_then(|d| now.checked_add_signed(d));
                        dt.and_then(|d| hm(chrono::Timelike::hour(&d), chrono::Timelike::minute(&d)).map(|t| (d.date(), Some(t))))
                    }
                    'd' if n <= 3650 => add_days(today, n).map(|d| (d, None)),
                    'w' if n <= 520 => add_days(today, n * 7).map(|d| (d, None)),
                    'M' if n <= 120 => today.checked_add_months(Months::new(n as u32)).map(|d| (d, None)),
                    _ => None,
                };
                if let Some((d, t)) = rel {
                    date = Some(d);
                    time = t;
                    explicit = true;
                    p.mark(i, 3);
                    break;
                }
            }
        }
    }
    if date.is_none() {
        for i in 0..p.t.len() {
            if let Some(h) = p.date_at(i) {
                date = Some(h.date);
                if time.is_none() {
                    time = h.part;
                }
                p.mark(i, h.n);
                break;
            }
        }
    }
    if time.is_none() || !explicit {
        for i in 0..p.t.len() {
            if let Some(h) = p.time_at(i) {
                time = Some(h.time);
                midnight = h.midnight;
                if let Some(nt) = h.note {
                    notes.push(nt);
                }
                p.mark(i, h.n);
                break;
            }
        }
    }
    if time.is_none() {
        time = reminder_time; // "remind me at 4" with nothing else: the task is at 4
    }

    // resolve
    if midnight {
        date = date.unwrap_or(today).succ_opt();
        explicit = true;
    }
    if let Some(rule) = &repeat {
        if date.is_none() {
            let first = repeat::first_on_or_after(rule, today);
            date = Some(first);
            if let Some(t) = time {
                if first.and_time(t) <= now {
                    date = repeat::next_date(rule, first, None).or(Some(first));
                }
            }
        }
    } else if date.is_none() && time.is_some() && !explicit {
        let t = time.unwrap_or_default();
        if today.and_time(t) <= now {
            date = today.succ_opt();
            notes.push("That time has passed today, so it is set for tomorrow.".into());
        } else {
            date = Some(today);
        }
    }
    if let (Some(Repeat::Monthly(0)), Some(d)) = (&repeat, date) {
        repeat = Some(Repeat::Monthly(d.day()));
    }
    if until.is_none() {
        if let (Some((n, u)), Some(d)) = (span, date) {
            let end = match u {
                'd' => add_days(d, n as i64 - 1),
                'w' => add_days(d, n as i64 * 7 - 1),
                _ => d.checked_add_months(Months::new(n)).and_then(|x| add_days(x, -1)),
            };
            until = end;
        }
    }
    if repeat.is_some() && date.is_none() {
        repeat = None;
    }

    // reminder given as a clock time: turn it into minutes before the due time
    if let (Some(rt), Some(d), Some(t)) = (reminder_time, date, time) {
        let diff = (d.and_time(t) - d.and_time(rt)).num_minutes();
        if diff > 0 {
            reminder_min = Some(diff);
        } else {
            notes.push("The reminder time is not before the task, so the default reminder is used.".into());
        }
    }

    // title: drop dangling words like "at", "on", "by" next to a removed date/time
    for u in 0..p.t.len() {
        if !p.t[u].used {
            continue;
        }
        let mut q = u;
        while q > 0 {
            let k = q - 1;
            if !p.free(k) {
                break;
            }
            let w = p.t[k].low.clone();
            if w == "the" && k > 0 && p.free(k - 1) && PREPS.contains(&p.t[k - 1].low.as_str()) {
                p.t[k].used = true;
                p.t[k - 1].used = true;
                q = k - 1;
            } else if PREPS.contains(&w.as_str()) {
                p.t[k].used = true;
                q = k;
            } else {
                break;
            }
        }
    }
    let mut words: Vec<(String, bool)> = p.t.iter().filter(|k| !k.used).map(|k| (k.orig.clone(), k.prot)).collect();
    let low = |w: &(String, bool)| if w.1 { String::new() } else { norm(&w.0) };
    // leading filler: "remind me to", "i need to", "don't forget to", "please"
    loop {
        let l: Vec<String> = words.iter().take(3).map(|w| low(w)).collect();
        let cut = match l.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["remind", "me", "to", ..] | ["i", "need", "to", ..] | ["i", "have", "to", ..] | ["don't", "forget", "to", ..] => 3,
            ["remind", "me", ..] => 2,
            ["please", ..] | ["to", ..] | ["and", ..] => 1,
            _ => 0,
        };
        if cut == 0 {
            break;
        }
        words.drain(..cut);
    }
    while words.last().map_or(false, |w| matches!(low(w).as_str(), "to" | "and" | "then")) {
        words.pop();
    }
    let mut title = words.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(" ");
    if title.chars().count() > MAX_TITLE_CHARS {
        title = title.chars().take(MAX_TITLE_CHARS).collect();
    }
    if let Some(c) = title.chars().next() {
        if c.is_lowercase() {
            title = c.to_uppercase().collect::<String>() + &title[c.len_utf8()..];
        }
    }
    if title.is_empty() {
        title = "Untitled task".into();
        notes.push("No title found. Type one in the title box.".into());
    }

    let is_event = time.is_some() && is_event_word(&title.to_lowercase());
    Parsed {
        title,
        date,
        time,
        priority,
        reminder_min,
        repeat,
        until,
        dur_min: dur_min.unwrap_or(if is_event { 60 } else { 30 }),
        is_event,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    /// Monday 2026-11-23, 15:00
    fn now() -> NaiveDateTime {
        d(2026, 11, 23).and_hms_opt(15, 0, 0).unwrap()
    }
    fn at(h: u32, m: u32) -> Option<NaiveTime> {
        hm(h, m)
    }
    fn p(s: &str) -> Parsed {
        parse(s, now())
    }
    fn p_at(s: &str, n: NaiveDateTime) -> Parsed {
        parse(s, n)
    }

    #[test]
    fn headline_sentence() {
        let r = p("Remind me to call John tomorrow at 5 PM");
        assert_eq!(r.title, "Call John");
        assert_eq!(r.date, Some(d(2026, 11, 24)));
        assert_eq!(r.time, at(17, 0));
        assert_eq!(r.priority, 1);
    }

    #[test]
    fn time_forms() {
        assert_eq!(p("call tomorrow 5pm").time, at(17, 0));
        assert_eq!(p("call tomorrow 5 p.m.").time, at(17, 0));
        assert_eq!(p("call tomorrow 17:00").time, at(17, 0));
        assert_eq!(p("call tomorrow at 5:30pm").time, at(17, 30));
        assert_eq!(p("call tomorrow at 9 AM").time, at(9, 0));
        assert_eq!(p("lunch tomorrow at noon").time, at(12, 0));
    }

    #[test]
    fn bare_time_rule_is_documented() {
        let r = p("call tomorrow at 5");
        assert_eq!(r.time, at(17, 0));
        assert!(!r.notes.is_empty());
        assert_eq!(p("call tomorrow at 9").time, at(9, 0));
    }

    #[test]
    fn midnight_is_start_of_next_day() {
        let r = p("submit tomorrow at midnight");
        assert_eq!(r.date, Some(d(2026, 11, 25)));
        assert_eq!(r.time, at(0, 0));
        assert_eq!(p("submit at midnight").date, Some(d(2026, 11, 24)));
    }

    #[test]
    fn past_bare_time_rolls_but_explicit_today_stays() {
        let r = p("call Sam at 9");
        assert_eq!(r.date, Some(d(2026, 11, 24)));
        assert_eq!(r.time, at(9, 0));
        let r = p("call Sam today at 9 AM");
        assert_eq!(r.date, Some(d(2026, 11, 23)));
        let r = p("call Sam at 5 pm");
        assert_eq!(r.date, Some(d(2026, 11, 23)));
    }

    #[test]
    fn weekdays_this_next_and_today_is_that_day() {
        // Monday now. Friday = 27th. Next Friday is 4 days away, so it is the 27th too
        assert_eq!(p("report friday").date, Some(d(2026, 11, 27)));
        assert_eq!(p("report this friday").date, Some(d(2026, 11, 27)));
        assert_eq!(p("report next friday").date, Some(d(2026, 11, 27)));
        // Tuesday 24th: Friday is 3 days away, so next Friday is the 27th. Wednesday 25th: only 2 days away, so 4 Dec
        let tue = d(2026, 11, 24).and_hms_opt(10, 0, 0).unwrap();
        let wed = d(2026, 11, 25).and_hms_opt(10, 0, 0).unwrap();
        assert_eq!(p_at("report next friday", tue).date, Some(d(2026, 11, 27)));
        assert_eq!(p_at("report next friday", wed).date, Some(d(2026, 12, 4)));
        assert_eq!(p_at("report friday", wed).date, Some(d(2026, 11, 27)));
        // Friday 2026-11-27 now
        let fri = d(2026, 11, 27).and_hms_opt(10, 0, 0).unwrap();
        assert_eq!(p_at("report friday", fri).date, Some(d(2026, 11, 27)));
        assert_eq!(p_at("report next friday", fri).date, Some(d(2026, 12, 4)));
        assert_eq!(p("report next week").date, Some(d(2026, 11, 30)));
    }

    #[test]
    fn relative_dates() {
        let r = p("stretch in 2 hours");
        assert_eq!(r.date, Some(d(2026, 11, 23)));
        assert_eq!(r.time, at(17, 0));
        assert_eq!(p("renew in 3 days").date, Some(d(2026, 11, 26)));
        assert_eq!(p("renew in 3 days").time, None);
        assert_eq!(p("pay rent end of month").date, Some(d(2026, 11, 30)));
    }

    #[test]
    fn named_and_numeric_dates() {
        assert_eq!(p("trip Jan 5").date, Some(d(2027, 1, 5)));
        assert_eq!(p("trip 5 Jan").date, Some(d(2027, 1, 5)));
        assert_eq!(p("trip on the 5th").date, Some(d(2026, 12, 5)));
        assert_eq!(p("trip on the 25th").date, Some(d(2026, 11, 25)));
        // day-first
        assert_eq!(p("party 12/11").date, Some(d(2027, 11, 12)));
        assert_eq!(p("party 12/11/2026").date, Some(d(2026, 11, 12)));
        assert_eq!(p("party 2026-12-20").date, Some(d(2026, 12, 20)));
    }

    #[test]
    fn month_end_and_leap_day() {
        let n = d(2026, 4, 10).and_hms_opt(8, 0, 0).unwrap(); // April has 30 days
        assert_eq!(p_at("pay on the 31st", n).date, Some(d(2026, 4, 30)));
        let n = d(2025, 3, 1).and_hms_opt(8, 0, 0).unwrap();
        assert_eq!(p_at("birthday Feb 29", n).date, Some(d(2028, 2, 29)));
        assert_eq!(p("meet 31 Apr").date, None); // not a real date: stays in the title
    }

    #[test]
    fn fixed_offset_now_is_zone_safe() {
        // The parser only sees local naive time, so any zone gives the same result for the same wall clock.
        let n = d(2026, 3, 8).and_hms_opt(23, 30, 0).unwrap();
        assert_eq!(p_at("call tomorrow at 9 am", n).date, Some(d(2026, 3, 9)));
    }

    #[test]
    fn priority_words() {
        assert_eq!(p("pay bill urgent").priority, 2);
        assert_eq!(p("high priority pay bill").priority, 2);
        assert_eq!(p("pay bill !!").priority, 2);
        assert_eq!(p("low water plants").priority, 0);
        assert_eq!(p("water plants").priority, 1);
        assert_eq!(p("pay bill urgent").title, "Pay bill");
        // "low" in the middle of a sentence is a normal word
        assert_eq!(p("fix low battery warning").priority, 1);
    }

    #[test]
    fn reminders() {
        let r = p("meeting tomorrow at 3pm remind me 30 minutes before");
        assert_eq!(r.reminder_min, Some(30));
        assert_eq!(r.time, at(15, 0));
        let r = p("call Ann tomorrow at 5pm remind me at 4");
        assert_eq!(r.reminder_min, Some(60));
        assert_eq!(r.time, at(17, 0));
        assert_eq!(p("call Ann tomorrow at 5pm").reminder_min, None);
    }

    #[test]
    fn repeats() {
        assert_eq!(p("water plants every day").repeat, Some(Repeat::Daily(1)));
        assert_eq!(p("standup every weekday at 9am").repeat, Some(Repeat::Weekdays));
        assert_eq!(p("gym every Monday and Wednesday at 6pm").repeat, Some(Repeat::Weekly { every: 1, days: vec![0, 2] }));
        assert_eq!(p("review every 2 weeks").repeat, Some(Repeat::Weekly { every: 2, days: vec![] }));
        assert_eq!(p("pay rent monthly on the 15th").repeat, Some(Repeat::Monthly(15)));
        let r = p("pay rent monthly on the 31st");
        assert_eq!(r.repeat, Some(Repeat::Monthly(31)));
        assert_eq!(r.date, Some(d(2026, 11, 30))); // first month end
        let r = p("water plants every day until Dec 20");
        assert_eq!(r.until, Some(d(2026, 12, 20)));
        assert_eq!(r.title, "Water plants");
        let r = p("yoga every Tuesday for 5 weeks");
        assert_eq!(r.date, Some(d(2026, 11, 24)));
        assert_eq!(r.until, Some(d(2026, 12, 28)));
        assert_eq!(r.title, "Yoga");
    }

    #[test]
    fn repeat_chip_text_reads_back() {
        for r in [
            Repeat::Daily(1),
            Repeat::Daily(3),
            Repeat::Weekdays,
            Repeat::Weekly { every: 1, days: vec![] },
            Repeat::Weekly { every: 2, days: vec![0, 2] },
            Repeat::Monthly(15),
        ] {
            assert_eq!(parse_repeat_phrase(&repeat::describe(&r), now()), Some(r));
        }
    }

    #[test]
    fn repeat_first_date_skips_past_time_today() {
        let r = p("water plants every day at 8 am");
        assert_eq!(r.date, Some(d(2026, 11, 24)));
        let r = p("water plants every day at 8 pm");
        assert_eq!(r.date, Some(d(2026, 11, 23)));
    }

    #[test]
    fn title_cleanup() {
        assert_eq!(p("call John").title, "Call John");
        assert_eq!(p("call John tomorrow").title, "Call John");
        let r = p("Call John about the 5 PM meeting");
        assert_eq!(r.title, "Call John about the 5 PM meeting");
        assert_eq!(r.time, None);
        assert_eq!(p("email \"lunch at 5pm\" to Bob tomorrow").title, "Email \"lunch at 5pm\" to Bob");
        assert_eq!(p("buy 5 apples").time, None);
        assert_eq!(p("Dentist appointment tomorrow at 4 pm").is_event, true);
        assert_eq!(p("call John").is_event, false);
    }

    #[test]
    fn empty_junk_long_and_unicode_do_not_panic() {
        assert_eq!(p("").title, "Untitled task");
        assert_eq!(p("   ").title, "Untitled task");
        assert_eq!(p("tomorrow").title, "Untitled task");
        let long = "a ".repeat(1000);
        assert!(p(&long).title.chars().count() <= MAX_TITLE_CHARS);
        let r = p("Fête 🎉 demain à 17h café ☕ tomorrow at 5pm");
        assert_eq!(r.time, at(17, 0));
        assert!(r.title.contains('🎉'));
        let _ = p("in 99999999999 days at 99:99 every 0 weeks 0/0/0 99th");
        let _ = p("every 365 weeks until 99/99/9999 for 520 months");
        let _ = p("\"unterminated quote at 5pm");
    }

    #[test]
    fn random_strings_never_panic() {
        let pieces = [
            "at", "5", "pm", "am", ":", "every", "week", "day", "tomorrow", "next", "friday", "12/11", "31st", "Feb", "29", "noon",
            "midnight", "!!", "in", "hours", "until", "for", "remind", "me", "before", "monthly", "on", "the", "🎉", "é", "\"", "0", "99999",
            "-", "/", "17:00", "a.m.", "of", "month", "end", "and", "mon", "low", "high", "priority", "@",
        ];
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        for _ in 0..3000 {
            let mut s = String::new();
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let n = (seed >> 33) % 12;
            for _ in 0..n {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                s.push_str(pieces[((seed >> 33) as usize) % pieces.len()]);
                s.push(' ');
            }
            let _ = parse(&s, now());
        }
    }

    #[test]
    fn reminder_text_says_when() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(9, 0, 0).unwrap();
        assert_eq!(parse("call John tomorrow at 5 PM", now).reminder_text().as_deref(), Some("Reminder: 10 min before"));
        assert_eq!(parse("pay rent tomorrow", now).reminder_text().as_deref(), Some("Reminder: 9:00 AM that day"));
        assert_eq!(parse("buy milk", now).reminder_text(), None);
    }
}