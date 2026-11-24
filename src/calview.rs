//! The calendar page: Month, Week, Day and Agenda. Server-rendered HTML; assets/app.js adds the mouse work
//! (drag a block, drag its bottom edge). Every piece of user text goes through `esc()`.
use crate::extra::{layout_columns, week_start};
use crate::store::Item;
use crate::view::{esc, page};
use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

pub const VIEWS: [(&str, &str); 4] = [("month", "Month"), ("week", "Week"), ("day", "Day"), ("agenda", "Agenda")];

fn ds(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn tlabel(t: NaiveTime) -> String {
    t.format("%-I:%M %p").to_string()
}

fn link(view: &str, d: NaiveDate) -> String {
    format!("/calendar?view={view}&amp;date={}", ds(d))
}

fn color(it: &Item) -> String {
    format!("c-{}", it.label_keys().first().copied().unwrap_or("none"))
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap_or(d)
}

fn shift_month(d: NaiveDate, by: i32) -> NaiveDate {
    let f = first_of_month(d);
    let m = Months::new(by.unsigned_abs());
    (if by >= 0 { f.checked_add_months(m) } else { f.checked_sub_months(m) }).unwrap_or(f)
}

/// "November 2026", "23 - 29 Nov 2026", "Tuesday 24 November 2026".
pub fn title_for(view: &str, date: NaiveDate) -> String {
    match view {
        "month" => date.format("%B %Y").to_string(),
        "week" => {
            let a = week_start(date);
            let b = a + Duration::days(6);
            if a.year() != b.year() {
                format!("{} - {}", a.format("%-d %b %Y"), b.format("%-d %b %Y"))
            } else if a.month() != b.month() {
                format!("{} - {}", a.format("%-d %b"), b.format("%-d %b %Y"))
            } else {
                format!("{} - {}", a.format("%-d"), b.format("%-d %b %Y"))
            }
        }
        "day" => date.format("%A %-d %B %Y").to_string(),
        _ => format!("{} - {}", date.format("%-d %b"), (date + Duration::days(29)).format("%-d %b %Y")),
    }
}

fn step(view: &str, date: NaiveDate, dir: i32) -> NaiveDate {
    match view {
        "month" => shift_month(date, dir),
        "week" => date + Duration::days(7 * dir as i64),
        "day" => date + Duration::days(dir as i64),
        _ => date + Duration::days(30 * dir as i64),
    }
}

fn on_day<'a>(items: &[&'a Item], d: NaiveDate) -> Vec<&'a Item> {
    let mut v: Vec<&Item> = items.iter().copied().filter(|i| i.at_local().map_or(false, |a| a.date() == d)).collect();
    v.sort_by_key(|i| (i.has_time, i.at, i.id));
    v
}

fn range_text(it: &Item) -> String {
    match it.at_local() {
        Some(a) if it.has_time => format!("{} - {}", tlabel(a.time()), tlabel((a + Duration::minutes(it.dur_min)).time())),
        _ => "All day".into(),
    }
}

fn mini_month(view: &str, date: NaiveDate, today: NaiveDate, busy: &[&Item]) -> String {
    let first = first_of_month(date);
    let start = week_start(first);
    let (lo, hi) = match view {
        "week" => (week_start(date), week_start(date) + Duration::days(6)),
        "day" => (date, date),
        "agenda" => (date, date + Duration::days(29)),
        _ => (first, shift_month(first, 1) - Duration::days(1)),
    };
    let go = if view == "month" { "day" } else { view };
    let mut cells = String::new();
    for h in ["M", "T", "W", "T", "F", "S", "S"] {
        cells.push_str(&format!("<span class=\"mh\">{h}</span>"));
    }
    let last = shift_month(first, 1) - Duration::days(1);
    let weeks = (week_start(last) - start).num_days() / 7 + 1;
    for k in 0..weeks * 7 {
        let d = start + Duration::days(k);
        let mut cls = String::from("md");
        if d.month() != first.month() {
            cls.push_str(" out");
        }
        if d == today {
            cls.push_str(" today");
        }
        if d >= lo && d <= hi {
            cls.push_str(" in");
        }
        if busy.iter().any(|i| i.at_local().map_or(false, |a| a.date() == d)) {
            cls.push_str(" has");
        }
        cells.push_str(&format!("<a class=\"{cls}\" href=\"{}\" aria-label=\"{}\">{}</a>", link(go, d), d.format("%A %-d %B"), d.day()));
    }
    format!(
        "<div class=\"mini\"><div class=\"mini-head\"><b>{}</b><span><a class=\"mnav\" href=\"{}\" aria-label=\"Previous month\">&#8249;</a><a class=\"mnav\" href=\"{}\" aria-label=\"Next month\">&#8250;</a></span></div><div class=\"mini-grid\">{cells}</div></div>",
        date.format("%B %Y"),
        link(view, shift_month(date, -1)),
        link(view, shift_month(date, 1)),
    )
}

fn month_view(items: &[&Item], date: NaiveDate, today: NaiveDate) -> String {
    let first = first_of_month(date);
    let start = week_start(first);
    let last = shift_month(first, 1) - Duration::days(1);
    let weeks = ((week_start(last) - start).num_days() / 7 + 1) as i64;
    let mut out = String::from("<div class=\"mg\" role=\"grid\">");
    for n in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        out.push_str(&format!("<div class=\"mdow\">{n}</div>"));
    }
    for k in 0..weeks * 7 {
        let d = start + Duration::days(k);
        let mine = on_day(items, d);
        let mut cls = String::from("mcell");
        if d.month() != first.month() {
            cls.push_str(" out");
        }
        if d == today {
            cls.push_str(" today");
        }
        let pills: String = mine
            .iter()
            .take(3)
            .map(|i| {
                let t = if i.has_time { format!("<span class=\"mt\">{}</span> ", i.at_local().map(|a| a.format("%-I:%M").to_string()).unwrap_or_default()) } else { String::new() };
                format!("<div class=\"mpill pdrag {}\" data-id=\"{}\" data-hm=\"{hm}\" tabindex=\"0\" role=\"button\" title=\"{tip}\" aria-label=\"Open card: {t2}\">{t}{t2}</div>", color(i), i.id, tip = esc(&format!("{}, {}", i.title, range_text(i))), hm = if i.has_time { i.at_local().map(|a| a.format("%H:%M").to_string()).unwrap_or_default() } else { String::new() }, t2 = esc(&i.title))
            })
            .collect();
        let more = if mine.len() > 3 { format!("<a class=\"mmore\" href=\"{}\">+{} more</a>", link("day", d), mine.len() - 3) } else { String::new() };
        out.push_str(&format!(
            "<div class=\"{cls}\" data-date=\"{}\" role=\"gridcell\"><a class=\"mdate\" href=\"{}\" aria-label=\"Open {}\">{}</a>{pills}{more}</div>",
            ds(d),
            link("day", d),
            d.format("%A %-d %B"),
            d.day()
        ));
    }
    out.push_str("</div>");
    out
}

/// Groups of blocks that overlap in time, chained (A overlaps B, B overlaps C = one group).
fn overlap_clusters(ev: &[(i64, i64, i64)]) -> Vec<Vec<i64>> {
    let mut sorted = ev.to_vec();
    sorted.sort_by_key(|e| (e.1, e.2, e.0));
    let mut out: Vec<Vec<i64>> = Vec::new();
    let mut end = i64::MIN;
    for (id, s, e) in sorted {
        if s >= end || out.is_empty() {
            out.push(Vec::new());
            end = e;
        }
        end = end.max(e);
        if let Some(c) = out.last_mut() {
            c.push(id);
        }
    }
    out
}

fn time_grid(items: &[&Item], first: NaiveDate, days: i64, today: NaiveDate, now: NaiveDateTime) -> String {
    let mut head = String::from("<div class=\"tg-corner\"></div>");
    let mut allday = String::from("<div class=\"tg-label\">all-day</div>");
    let mut cols = String::from("<div class=\"tg-hours\" aria-hidden=\"true\">");
    for h in 0..24u32 {
        let t = NaiveTime::from_hms_opt(h, 0, 0).unwrap_or_default();
        cols.push_str(&format!("<span style=\"top:calc(var(--hh)*{h})\">{}</span>", if h == 0 { String::new() } else { tlabel(t) }));
    }
    cols.push_str("</div>");
    for k in 0..days {
        let d = first + Duration::days(k);
        let tcls = if d == today { " today" } else { "" };
        head.push_str(&format!(
            "<a class=\"tg-dh{tcls}\" href=\"{}\"><span class=\"dow\">{}</span><span class=\"dnum\">{}</span></a>",
            link("day", d),
            d.format("%a"),
            d.day()
        ));
        let mine = on_day(items, d);
        let pills: String = mine
            .iter()
            .filter(|i| !i.has_time)
            .map(|i| format!("<div class=\"adpill pdrag {}\" data-id=\"{}\" tabindex=\"0\" role=\"button\" title=\"{t}, All day\" aria-label=\"Open card: {t}\">{t}</div>", color(i), i.id, t = esc(&i.title)))
            .collect();
        allday.push_str(&format!("<div class=\"adcell\" data-date=\"{}\">{pills}</div>", ds(d)));
        let timed: Vec<&Item> = mine.iter().copied().filter(|i| i.has_time).collect();
        let ev: Vec<(i64, i64, i64)> = timed
            .iter()
            .filter_map(|i| {
                let a = i.at_local()?;
                let s = (a.hour() * 60 + a.minute()) as i64;
                Some((i.id, s, (s + i.dur_min).min(1440).max(s + 5)))
            })
            .collect();
        let lay = layout_columns(&ev);
        // Week view only: at most 2 side-by-side columns (1 on a phone, by CSS); the rest become a "+N" chip.
        let cap = if days > 1 { 2 } else { usize::MAX };
        let mut blocks = String::new();
        let mut chips = String::new();
        if days > 1 {
            for ids in overlap_clusters(&ev) {
                let col_of = |id: i64| lay.iter().find(|x| x.0 == id).map_or(0, |x| x.1);
                let start_of = |id: i64| ev.iter().find(|x| x.0 == id).map_or(0, |x| x.1);
                let hid = |min_col: usize| ids.iter().copied().filter(|&id| col_of(id) >= min_col).collect::<Vec<_>>();
                for (min_col, cls) in [(2usize, "mored"), (1usize, "morep")] {
                    let h = hid(min_col);
                    if let Some(top) = h.iter().map(|&id| start_of(id)).min() {
                        chips.push_str(&format!(
                            "<a class=\"more {cls}\" href=\"{}\" style=\"top:calc(var(--hh)*{:.4})\" title=\"{n} more at this time. Open the day.\" aria-label=\"{n} more at this time. Open the day.\">+{n}<span class=\"mw\"> more</span></a>",
                            link("day", d),
                            top as f64 / 60.0,
                            n = h.len()
                        ));
                    }
                }
            }
        }
        for i in &timed {
            let Some(&(_, s, e)) = ev.iter().find(|x| x.0 == i.id) else { continue };
            let Some(&(_, col, n)) = lay.iter().find(|x| x.0 == i.id) else { continue };
            if col >= cap {
                continue;
            }
            let w = 100.0 / n.min(cap) as f64;
            let more = if days > 1 && col >= 1 { " h1" } else { "" };
            
            let short = if e - s <= 45 { " short" } else { "" };
            let tip = esc(&format!("{}, {}", i.title, range_text(i)));
            let sub = if short.is_empty() { esc(&range_text(i)) } else { esc(&format!("{}", i.at_local().map(|a| tlabel(a.time())).unwrap_or_default())) };
            let fixed = if i.is_event() { " fx" } else { "" };
            blocks.push_str(&format!(
                "<div class=\"evb pdrag {c}{short}{fixed}{more}\" data-id=\"{id}\" data-dur=\"{dur}\" data-start=\"{s}\" tabindex=\"0\" role=\"button\" title=\"{tip}\" aria-label=\"Open card: {t}, {r}\" style=\"top:calc(var(--hh)*{top:.4});height:max(22px, calc(var(--hh)*{hh:.4} - 2px));left:{l:.3}%;width:calc({w:.3}% - 3px)\"><b class=\"evt\">{t}</b><span class=\"evtime\">{sub}</span><i class=\"rz\" title=\"Drag to change the length\" aria-hidden=\"true\"></i></div>",
                c = color(i),
                id = i.id,
                dur = i.dur_min,
                t = esc(&i.title),
                r = esc(&range_text(i)),
                top = s as f64 / 60.0,
                hh = (e - s) as f64 / 60.0,
                l = w * col as f64,
                w = w,
            ));
        }
        let nowline = if d == now.date() {
            format!("<div class=\"nowline\" style=\"top:calc(var(--hh)*{:.4})\"><i></i></div>", (now.hour() * 60 + now.minute()) as f64 / 60.0)
        } else {
            String::new()
        };
        cols.push_str(&format!("<div class=\"dcol{tcls}\" data-date=\"{}\">{nowline}{blocks}{chips}</div>", ds(d)));
    }
    format!(
        "<div class=\"tg-wrap\"><div class=\"tg\" style=\"--n:{days}\"><div class=\"tg-head\">{head}</div><div class=\"tg-allday\">{allday}</div><div class=\"tg-scroll\" id=\"tg-scroll\"><div class=\"tg-body\">{cols}</div></div></div></div>"
    )
}

fn agenda_view(items: &[&Item], loose: &[&Item], date: NaiveDate, today: NaiveDate) -> String {
    let mut out = String::from("<div class=\"agenda\">");
    let mut shown = 0;
    for k in 0..30 {
        let d = date + Duration::days(k);
        let mine = on_day(items, d);
        if mine.is_empty() && d != today {
            continue;
        }
        shown += 1;
        let tag = if d == today { " <span class=\"chip today-tag\">Today</span>" } else { "" };
        let rows: String = if mine.is_empty() {
            "<li class=\"ag-empty\">Nothing planned.</li>".into()
        } else {
            mine.iter()
                .map(|i| {
                    format!(
                        "<li class=\"ag-item pdrag {}\" data-id=\"{}\" tabindex=\"0\" role=\"button\" title=\"{t}, {r}\" aria-label=\"Open card: {t}\"><span class=\"ag-time\">{}</span><span class=\"ag-title\">{t}</span></li>",
                        color(i),
                        i.id,
                        esc(&if i.has_time { range_text(i) } else { "All day".into() }),
                        t = esc(&i.title),
                        r = esc(&if i.has_time { range_text(i) } else { "All day".into() })
                    )
                })
                .collect()
        };
        out.push_str(&format!("<section class=\"ag-day{}\"><h2>{}{tag}</h2><ul>{rows}</ul></section>", if d == today { " today" } else { "" }, d.format("%A %-d %B")));
    }
    if shown == 0 {
        out.push_str("<p class=\"muted\">Nothing planned in the next 30 days.</p>");
    }
    if !loose.is_empty() {
        let rows: String = loose
            .iter()
            .take(20)
            .map(|i| format!("<li class=\"ag-item {}\" data-id=\"{}\" tabindex=\"0\" role=\"button\" aria-label=\"Open card: {t}\"><span class=\"ag-time\">No date</span><span class=\"ag-title\">{t}</span></li>", color(i), i.id, t = esc(&i.title)))
            .collect();
        out.push_str(&format!("<section class=\"ag-day\"><h2>Not scheduled yet</h2><ul>{rows}</ul></section>"));
    }
    out.push_str("</div>");
    out
}

/// `view` is month, week, day or agenda (anything else reads as week).
pub fn calendar_page(board: &str, items: &[Item], view: &str, date: NaiveDate, now: NaiveDateTime) -> String {
    let view = VIEWS.iter().map(|v| v.0).find(|v| *v == view).unwrap_or("week");
    let today = now.date();
    let open: Vec<&Item> = items.iter().filter(|i| !i.done && !i.archived).collect();
    let dated: Vec<&Item> = open.iter().copied().filter(|i| i.at.is_some()).collect();
    let loose: Vec<&Item> = open.iter().copied().filter(|i| i.at.is_none()).collect();
    let main = match view {
        "month" => month_view(&dated, date, today),
        "week" => time_grid(&dated, week_start(date), 7, today, now),
        "day" => time_grid(&dated, date, 1, today, now),
        _ => agenda_view(&dated, &loose, date, today),
    };
    let seg: String = VIEWS
        .iter()
        .map(|(v, name)| format!("<a href=\"{}\"{}>{name}</a>", link(v, date), if *v == view { " aria-current=\"true\"" } else { "" }))
        .collect();
    let un: String = if loose.is_empty() {
        "<p class=\"muted\">Every card has a date.</p>".into()
    } else {
        let rows: String = loose
            .iter()
            .take(12)
            .map(|i| format!("<div class=\"un-item pdrag {}\" data-id=\"{}\" tabindex=\"0\" role=\"button\" aria-label=\"Open card: {t}\">{t}</div>", color(i), i.id, t = esc(&i.title)))
            .collect();
        let extra = if loose.len() > 12 { format!("<p class=\"muted\">+{} more on the board</p>", loose.len() - 12) } else { String::new() };
        format!("<div class=\"un-list\">{rows}</div>{extra}")
    };
    let body = format!(
        "<h1 class=\"sr\">Calendar</h1><div class=\"calwrap\"><aside class=\"calside\" aria-label=\"Calendar sidebar\">{mini}<h2 class=\"un-h\">Not scheduled</h2><p class=\"muted\">Drag a card onto the calendar.</p>{un}</aside>
<section class=\"calmain\"><div class=\"caltop\"><div class=\"row\"><a class=\"btn\" href=\"{today_l}\">Today</a><a class=\"btn arrow\" href=\"{prev}\" aria-label=\"Previous\">&#8249;</a><a class=\"btn arrow\" href=\"{next}\" aria-label=\"Next\">&#8250;</a><h2 class=\"caltitle\" aria-live=\"polite\">{title}</h2></div>
<nav class=\"seg\" aria-label=\"Calendar view\">{seg}</nav></div>{main}</section></div>",
        mini = mini_month(view, date, today, &dated),
        today_l = link(view, today),
        prev = link(view, step(view, date, -1)),
        next = link(view, step(view, date, 1)),
        title = esc(&title_for(view, date)),
    );
    page(board, "Calendar", "/calendar", &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::to_utc;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 24).unwrap().and_hms_opt(10, 30, 0).unwrap()
    }
    fn item(id: i64, title: &str, day: u32, hm: Option<(u32, u32)>, dur: i64, labels: &str) -> Item {
        let d = NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        let t = hm.map_or(NaiveTime::default(), |(h, m)| NaiveTime::from_hms_opt(h, m, 0).unwrap());
        Item {
            id,
            title: title.into(),
            kind: "task".into(),
            at: Some(to_utc(d.and_time(t))),
            has_time: hm.is_some(),
            dur_min: dur,
            deadline: None,
            priority: 1,
            reminder_min: None,
            remind_at: None,
            fired: false,
            acked: false,
            repeat: None,
            until: None,
            done: false,
            sample: false,
            list_id: 1,
            pos: 0,
            notes: String::new(),
            labels: labels.into(),
            checklist: vec![],
            archived: false,
            cycle: String::new(),
        }
    }
    fn today() -> NaiveDate {
        now().date()
    }

    #[test]
    fn titles_read_like_a_calendar() {
        assert_eq!(title_for("month", today()), "November 2026");
        assert_eq!(title_for("week", today()), "23 - 29 Nov 2026");
        assert_eq!(title_for("week", NaiveDate::from_ymd_opt(2026, 11, 30).unwrap()), "30 Nov - 6 Dec 2026");
        assert_eq!(title_for("day", today()), "Tuesday 24 November 2026");
        let h = calendar_page("b", &[], "month", today(), now());
        assert!(h.contains("November 2026") && h.contains("Today</a>") && h.contains("aria-label=\"Previous\"") && h.contains("aria-label=\"Next\""));
        assert!(h.contains("href=\"/calendar?view=month&amp;date=2026-12-01\""), "next goes to December");
    }

    #[test]
    fn segmented_switch_has_four_views_and_marks_the_current_one() {
        let h = calendar_page("b", &[], "day", today(), now());
        for v in ["Month", "Week", "Day", "Agenda"] {
            assert!(h.contains(&format!(">{v}</a>")), "{v}");
        }
        assert!(h.contains("aria-current=\"true\">Day</a>"));
    }

    #[test]
    fn month_shows_three_pills_then_plus_n_more() {
        let items: Vec<Item> = (1..=5).map(|i| item(i, &format!("Card {i}"), 24, Some((8 + i as u32, 0)), 30, "green")).collect();
        let h = calendar_page("b", &items, "month", today(), now());
        assert_eq!(h.matches("class=\"mpill ").count(), 3);
        assert!(h.contains("+2 more"));
        assert!(h.contains("c-green"));
        assert!(h.contains("data-date=\"2026-11-24\""));
    }

    #[test]
    fn week_caps_columns_at_two_and_adds_a_more_chip() {
        let items = vec![item(1, "A", 24, Some((11, 0)), 45, ""), item(2, "B", 24, Some((11, 0)), 45, ""), item(3, "C", 24, Some((11, 0)), 45, ""), item(4, "D", 24, Some((11, 0)), 45, "")];
        let w = calendar_page("b", &items, "week", today(), now());
        assert_eq!(w.matches("class=\"evb ").count(), 2, "only two blocks are drawn");
        assert!(w.contains("class=\"more mored\"") && w.contains("+2<span"));
        assert!(w.contains("class=\"more morep\"") && w.contains("+3<span"));
        let d = calendar_page("b", &items, "day", today(), now());
        assert_eq!(d.matches("class=\"evb ").count(), 4, "the day view shows every block");
        assert!(!d.contains("class=\"more "));
    }

    #[test]
    fn week_blocks_have_position_length_and_share_width_when_they_overlap() {
        let items = vec![item(1, "A", 24, Some((9, 0)), 60, "red"), item(2, "B", 24, Some((9, 30)), 60, ""), item(3, "C", 25, Some((14, 0)), 45, "blue")];
        let h = calendar_page("b", &items, "week", today(), now());
        assert!(h.contains("top:calc(var(--hh)*9.0000);height:max(22px, calc(var(--hh)*1.0000 - 2px));left:0.000%;width:calc(50.000% - 3px)"), "{h}");
        assert!(h.contains("left:50.000%"));
        assert!(h.contains("height:max(22px, calc(var(--hh)*0.7500 - 2px));left:0.000%;width:calc(100.000% - 3px)"));
        assert!(h.contains("class=\"nowline\"") && h.contains("class=\"tg-allday\""));
        assert!(h.contains("data-dur=\"60\"") && h.contains("<i class=\"rz\""));
        assert_eq!(h.matches("class=\"dcol").count(), 7);
    }

    #[test]
    fn short_blocks_keep_their_start_get_a_one_line_label_and_every_view_has_a_tooltip() {
        let items = vec![item(1, "Call John", 24, Some((17, 0)), 15, ""), item(2, "Long one", 24, Some((9, 0)), 90, "")];
        let h = calendar_page("b", &items, "week", today(), now());
        assert!(h.contains("evb pdrag c-none short") && h.contains("top:calc(var(--hh)*17.0000);height:max(22px, calc(var(--hh)*0.2500 - 2px))"), "{h}");
        assert!(h.contains("<b class=\"evt\">Call John</b><span class=\"evtime\">5:00 PM</span>"));
        assert!(h.contains("<span class=\"evtime\">9:00 AM - 10:30 AM</span>"), "long blocks keep the range");
        for v in ["week", "day", "month", "agenda"] {
            let h = calendar_page("b", &items, v, today(), now());
            assert!(h.contains("title=\"Call John, 5:00 PM - 5:15 PM\""), "{v}");
        }
        let all = vec![item(3, "Pay rent", 24, None, 30, "")];
        assert!(calendar_page("b", &all, "week", today(), now()).contains("title=\"Pay rent, All day\""));
    }

    #[test]
    fn all_day_cards_sit_in_the_top_row_and_calendar_text_is_escaped() {
        let evil = "<script>alert(1)</script>";
        let items = vec![item(1, evil, 24, None, 30, "red"), item(2, evil, 24, Some((9, 0)), 30, "")];
        for v in ["month", "week", "day", "agenda"] {
            let h = calendar_page(evil, &items, v, today(), now());
            assert!(!h.contains("<script>alert(1)"), "{v}");
            assert!(h.contains("&lt;script&gt;"), "{v}");
        }
        let h = calendar_page("b", &items, "day", today(), now());
        assert_eq!(h.matches("class=\"adpill ").count(), 1);
        assert_eq!(h.matches("class=\"dcol").count(), 1);
    }

    #[test]
    fn agenda_lists_days_and_unscheduled_cards() {
        let mut loose = item(9, "Read 20 pages", 24, None, 30, "");
        loose.at = None;
        let items = vec![item(1, "Dentist", 26, Some((16, 30)), 60, ""), loose];
        let h = calendar_page("b", &items, "agenda", today(), now());
        assert!(h.contains("Thursday 26 November") && h.contains("4:30 PM - 5:30 PM") && h.contains("Nothing planned."));
        assert!(h.contains("Not scheduled yet") && h.contains("Read 20 pages"));
    }
}

