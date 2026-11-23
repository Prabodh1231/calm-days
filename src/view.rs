//! HTML pages. Plain `format!` plus `esc()`. Every piece of user text goes through `esc()`.
use crate::approval::Change;
use crate::planner::{self, NowTask};
use crate::repeat;
use crate::store::{Completion, Item, UndoRow};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

/// Escape text for HTML (also safe inside double-quoted attributes).
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

const PRIO: [&str; 3] = ["Low", "Medium", "High"];

fn day_label(d: NaiveDate, today: NaiveDate) -> String {
    match (d - today).num_days() {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        _ => d.format("%a %-d %b").to_string(),
    }
}

fn time_label(t: NaiveTime) -> String {
    t.format("%-I:%M %p").to_string()
}

pub fn dt_label(d: NaiveDateTime, now: NaiveDateTime) -> String {
    format!("{}, {}", day_label(d.date(), now.date()), time_label(d.time()))
}

pub fn when_label(it: &Item, now: NaiveDateTime) -> String {
    match it.at_local() {
        None => "No date".into(),
        Some(a) if it.has_time => dt_label(a, now),
        Some(a) => format!("{} (all day)", day_label(a.date(), now.date())),
    }
}

pub fn page(title: &str, active: &str, body: &str) -> String {
    let nav = [("/", "Today"), ("/calendar", "Calendar"), ("/plan", "Plan"), ("/summary", "Summary")]
        .iter()
        .map(|(h, l)| {
            let cur = if *h == active { " aria-current=\"page\"" } else { "" };
            format!("<a href=\"{h}\"{cur}>{l}</a>")
        })
        .collect::<String>();
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - Calm Days</title>
<script>document.documentElement.dataset.theme=localStorage.getItem('theme')||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');</script>
<link rel="stylesheet" href="/static/style.css">
</head><body>
<header class="top"><a class="brand" href="/">Calm Days</a>
<nav aria-label="Main">{nav}</nav>
<button id="theme" type="button" aria-label="Switch between light and dark mode">Dark mode</button></header>
<div id="banner" role="status" aria-live="polite"></div>
<main>{body}</main>
<dialog id="move-dialog"><form method="dialog"><h2>Move to...</h2>
<p id="move-title" class="muted"></p>
<label for="move-when">New date and time</label>
<input id="move-when" type="datetime-local" required>
<div class="row"><button id="move-ok" type="button" class="primary">Move</button><button id="move-cancel" type="button">Cancel</button></div>
</form></dialog>
<div id="toast" role="status" aria-live="polite" hidden></div>
<script src="/static/app.js"></script>
</body></html>"##,
        title = esc(title),
        nav = nav,
        body = body
    )
}

fn badge(it: &Item) -> String {
    let mut s = format!("<span class=\"badge p{}\">{}</span>", it.priority.clamp(0, 2), PRIO[it.priority.clamp(0, 2) as usize]);
    if it.is_event() {
        s.push_str("<span class=\"badge ev\">Fixed event</span>");
    }
    if let Some(r) = &it.repeat {
        s.push_str(&format!("<span class=\"badge rep\">{}</span>", esc(&repeat::describe(r))));
    }
    s
}

fn item_row(it: &Item, now: NaiveDateTime) -> String {
    let at = it.at_local().map(|a| a.format("%Y-%m-%dT%H:%M").to_string()).unwrap_or_default();
    let t = esc(&it.title);
    let overdue = if it.is_overdue(now) { " overdue" } else { "" };
    format!(
        r#"<li class="task{overdue}" draggable="true" data-id="{id}"><button type="button" class="done" data-id="{id}" aria-label="Mark done: {t}">Done</button>
<div class="main"><span class="title">{t}</span><span class="when">{when}</span><span class="badges">{badge}</span></div>
<div class="acts"><button type="button" class="move" data-id="{id}" data-at="{at}" data-title="{t}" aria-label="Move {t} to another time">Move to...</button>
<button type="button" class="del" data-id="{id}" aria-label="Delete {t}">Delete</button></div></li>"#,
        id = it.id,
        when = esc(&when_label(it, now)),
        badge = badge(it),
    )
}

fn list_block(title: &str, note: &str, items: &[&Item], now: NaiveDateTime, extra: &str) -> String {
    if items.is_empty() {
        return String::new();
    }
    let rows: String = items.iter().map(|i| item_row(i, now)).collect();
    format!(
        "<section class=\"card\"><div class=\"row between\"><h2>{}</h2>{}</div><p class=\"muted\">{}</p><ul class=\"tasks\">{}</ul></section>",
        esc(title),
        extra,
        esc(note),
        rows
    )
}

pub fn now_task_list(items: &[Item]) -> Vec<NowTask> {
    items
        .iter()
        .filter(|i| !i.is_event() && !i.done)
        .map(|i| NowTask { id: i.id, at: i.at_local(), has_time: i.has_time, dur: i.dur_min, priority: i.priority })
        .collect()
}

pub fn today_page(items: &[Item], done_today: usize, pending: usize, now: NaiveDateTime) -> String {
    let today = now.date();
    let open: Vec<&Item> = items.iter().filter(|i| !i.done).collect();
    let overdue: Vec<&Item> = open.iter().copied().filter(|i| i.is_overdue(now)).collect();
    let on = |i: &&Item, lo: i64, hi: i64| {
        !i.is_overdue(now) && i.at_local().map_or(false, |a| {
            let n = (a.date() - today).num_days();
            n >= lo && n <= hi
        })
    };
    let todays: Vec<&Item> = open.iter().filter(|i| on(*i, 0, 0)).copied().collect();
    let week: Vec<&Item> = open.iter().filter(|i| on(*i, 1, 7)).copied().collect();
    let later: Vec<&Item> = open.iter().filter(|i| i.at_local().map_or(false, |a| (a.date() - today).num_days() > 7)).copied().collect();
    let nodate: Vec<&Item> = open.iter().filter(|i| i.at.is_none()).copied().collect();

    let now_card = match planner::do_now(now, &now_task_list(items)) {
        Some((id, why)) => {
            let it = items.iter().find(|i| i.id == id);
            match it {
                Some(it) => format!(
                    "<p class=\"big\">{}</p><p class=\"muted\">{} {}</p><button type=\"button\" class=\"done primary\" data-id=\"{}\" aria-label=\"Mark done: {}\">Done</button>",
                    esc(&it.title),
                    esc(&why),
                    esc(&when_label(it, now)),
                    it.id,
                    esc(&it.title)
                ),
                None => "<p class=\"big\">You're clear</p>".into(),
            }
        }
        None => "<p class=\"big\">You're clear</p><p class=\"muted\">Nothing needs you right now. Enjoy the quiet, or add something below.</p>".into(),
    };
    let stats = format!(
        "<div class=\"stats\"><div><b>{}</b><span>Open</span></div><div><b>{}</b><span>Due today</span></div><div class=\"{}\"><b>{}</b><span>Overdue</span></div><div><b>{}</b><span>Done today</span></div></div>",
        open.len(),
        todays.len(),
        if overdue.is_empty() { "" } else { "warn" },
        overdue.len(),
        done_today
    );
    let proposals = if pending > 0 {
        format!("<p class=\"notice\"><a href=\"/plan\">{pending} suggested change(s) are waiting for your approval.</a></p>")
    } else {
        String::new()
    };
    let resched = "<button type=\"button\" id=\"plan-missed\" class=\"primary\">Suggest new times</button>";
    let examples = [
        "Remind me to call John tomorrow at 5 PM",
        "Pay rent every month on the 1st !!",
        "Gym every Monday and Wednesday at 6 pm",
        "Send report by Friday high priority",
    ]
    .iter()
    .map(|e| format!("<button type=\"button\" class=\"example\" data-text=\"{}\">{}</button>", esc(e), esc(e)))
    .collect::<String>();

    let body = format!(
        r##"<section class="hero"><h1>Your day, in plain sentences</h1>
<p class="lead">Type what you need to do, the way you would tell a friend. Calm Days finds the date, time, priority, reminder and repeat, and puts it in your list and calendar.</p></section>
{proposals}
<section class="card add"><label for="text" class="big-label">What do you want to remember or plan?</label>
<div class="row"><input id="text" type="text" maxlength="500" autocomplete="off" placeholder="Try: Remind me to call John tomorrow at 5 PM"><button id="read" type="button" class="primary">Read it</button></div>
<p class="muted">Smart text reading, works offline. It follows simple rules (it is not an AI), so check the preview before you save. Numeric dates are day-first: 12/11 means 12 November.</p>
<div class="examples"><span class="muted">Examples:</span> {examples}</div>
<div id="preview" hidden><h2>Check and edit before saving</h2><form id="chips" class="chips">
<label>Title<input name="title" type="text" maxlength="200" required></label>
<label>Date<input name="date" type="date"></label>
<label>Time<input name="time" type="time"></label>
<label>Priority<select name="priority"><option value="0">Low</option><option value="1">Medium</option><option value="2">High</option></select></label>
<label>Reminder<select name="reminder_min"><option value="">Default (10 min before)</option><option value="5">5 min before</option><option value="30">30 min before</option><option value="60">1 hour before</option><option value="1440">1 day before</option><option value="-1">No reminder</option></select></label>
<label>Repeat<input name="repeat" type="text" placeholder="none, or: every week on mon and wed"></label>
<label>Repeat until<input name="until" type="date"></label>
<label>Length (minutes)<input name="dur_min" type="number" min="5" max="1440" value="30"></label>
<label>Type<select name="kind"><option value="task">Task</option><option value="event">Fixed event</option></select></label>
</form><ul id="notes" class="muted"></ul>
<div class="row"><button id="save" type="button" class="primary">Save</button><button id="discard" type="button">Cancel</button></div></div></section>
<section class="card"><h2>What should I do now?</h2>{now_card}</section>
<section class="card"><h2>Your day at a glance</h2>{stats}</section>
{overdue_block}{today_block}{week_block}{later_block}{nodate_block}
<section class="card"><h2>New here?</h2><p>Press the button to fill the app with an example day. You can undo every change.</p><button id="sample" type="button">Load sample day</button></section>
<section class="card"><h2>About reminders</h2><p class="muted">Reminders pop up while this app is open in a browser tab. Reminders that came due while it was closed show in a yellow bar when you return. They do not appear if the app or the browser is closed.</p>
<button id="notify" type="button">Turn on pop-up reminders</button></section>"##,
        overdue_block = list_block("Overdue", "These slipped past. Press 'Suggest new times' to get a plan you can approve.", &overdue, now, resched),
        today_block = list_block("Today", "", &todays, now, ""),
        week_block = list_block("Next 7 days", "", &week, now, ""),
        later_block = list_block("Later", "", &later, now, ""),
        nodate_block = list_block("No date yet", "Drag these onto the calendar, or use Move to...", &nodate, now, ""),
    );
    page("Today", "/", &body)
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub fn calendar_page(items: &[Item], week: bool, date: NaiveDate, now: NaiveDateTime) -> String {
    let (first, count) = if week { (monday_of(date), 7) } else { (date, 1) };
    let step = if week { 7 } else { 1 };
    let nav = format!(
        "<div class=\"row between\"><div class=\"row\"><a class=\"btn\" href=\"/calendar?view={v}&date={prev}\">Previous</a><a class=\"btn\" href=\"/calendar?view={v}&date={today}\">Today</a><a class=\"btn\" href=\"/calendar?view={v}&date={next}\">Next</a></div><div class=\"row\"><a class=\"btn{d_on}\" href=\"/calendar?view=day&date={cur}\">Day</a><a class=\"btn{w_on}\" href=\"/calendar?view=week&date={cur}\">Week</a></div></div>",
        v = if week { "week" } else { "day" },
        prev = (date - Duration::days(step)).format("%Y-%m-%d"),
        next = (date + Duration::days(step)).format("%Y-%m-%d"),
        today = now.date().format("%Y-%m-%d"),
        cur = date.format("%Y-%m-%d"),
        d_on = if week { "" } else { " on" },
        w_on = if week { " on" } else { "" },
    );
    let open: Vec<&Item> = items.iter().filter(|i| !i.done).collect();
    let mut days = String::new();
    for k in 0..count {
        let d = first + Duration::days(k);
        let cls = if d == now.date() { " today" } else { "" };
        let allday: String = open
            .iter()
            .filter(|i| !i.has_time && i.at_local().map_or(false, |a| a.date() == d))
            .map(|i| item_row(i, now))
            .collect();
        let mut rows = String::new();
        for h in 6..=21u32 {
            let cell: String = open
                .iter()
                .filter(|i| i.has_time)
                .filter_map(|i| i.at_local().map(|a| (i, a)))
                .filter(|(_, a)| a.date() == d && a.hour().clamp(6, 21) == h)
                .map(|(i, a)| {
                    format!(
                        "<div class=\"card-ev{ev}\" draggable=\"true\" data-id=\"{id}\"><b>{t}</b> {title}<button type=\"button\" class=\"move small\" data-id=\"{id}\" data-at=\"{at}\" data-title=\"{title}\" aria-label=\"Move {title} to another time\">Move to...</button></div>",
                        ev = if i.is_event() { " fixed" } else { "" },
                        id = i.id,
                        t = esc(&time_label(a.time())),
                        title = esc(&i.title),
                        at = a.format("%Y-%m-%dT%H:%M")
                    )
                })
                .collect();
            rows.push_str(&format!(
                "<div class=\"slot\" data-slot=\"{}T{:02}:00\"><span class=\"hour\">{}</span><div class=\"cell\">{}</div></div>",
                d.format("%Y-%m-%d"),
                h,
                time_label(NaiveTime::from_hms_opt(h, 0, 0).unwrap_or_default()),
                cell
            ));
        }
        let all = if allday.is_empty() { String::new() } else { format!("<ul class=\"tasks allday\">{allday}</ul>") };
        days.push_str(&format!(
            "<section class=\"day{cls}\"><h2>{}</h2>{all}<div class=\"slots\">{rows}</div></section>",
            esc(&format!("{} {}", day_label(d, now.date()), d.format("%-d %b")))
        ));
    }
    let loose: Vec<&Item> = open.iter().filter(|i| i.at.is_none()).copied().collect();
    let side = if loose.is_empty() {
        "<p class=\"muted\">Everything has a date.</p>".to_string()
    } else {
        format!("<ul class=\"tasks\">{}</ul>", loose.iter().map(|i| item_row(i, now)).collect::<String>())
    };
    let body = format!(
        "<h1>Calendar</h1><p class=\"muted\">Drag a task onto an hour. On a phone or with a keyboard, use <b>Move to...</b> instead.</p>{nav}<div class=\"cal {}\">{days}</div><section class=\"card\"><h2>Not on the calendar yet</h2>{side}</section>",
        if week { "week" } else { "dayview" }
    );
    page("Calendar", "/calendar", &body)
}

pub fn change_lines(c: &Change, now: NaiveDateTime) -> (String, Vec<String>) {
    match c {
        Change::Move { moves } | Change::Plan { moves } => {
            let name = if matches!(c, Change::Plan { .. }) { "Suggested plan" } else { "Move" };
            let lines = moves
                .iter()
                .map(|m| match m.from {
                    Some(f) => format!("Move \"{}\" from {} to {}", m.title, dt_label(f, now), dt_label(m.to, now)),
                    None => format!("Schedule \"{}\" at {}", m.title, dt_label(m.to, now)),
                })
                .collect();
            (name.into(), lines)
        }
        Change::Delete { titles, .. } => ("Delete".into(), titles.iter().map(|t| format!("Delete \"{t}\"")).collect()),
        Change::EditFuture { title, priority, id } => {
            let mut l = vec![format!("Change all future repeats of item #{id}")];
            if let Some(t) = title {
                l.push(format!("New title: \"{t}\""));
            }
            if let Some(p) = priority {
                l.push(format!("New priority: {}", PRIO[(*p).clamp(0, 2) as usize]));
            }
            ("Edit all future".into(), l)
        }
    }
}

pub fn plan_page(pending: &[(i64, Change)], undo: &[UndoRow], now: NaiveDateTime) -> String {
    let props = if pending.is_empty() {
        "<p class=\"muted\">No suggestions waiting. Press a button above to get one.</p>".to_string()
    } else {
        pending
            .iter()
            .map(|(id, c)| {
                let (name, lines) = change_lines(c, now);
                let li: String = lines.iter().map(|l| format!("<li>{}</li>", esc(l))).collect();
                format!(
                    "<article class=\"card prop\"><h3>{}</h3><ul>{li}</ul><div class=\"row\"><button type=\"button\" class=\"approve primary\" data-pid=\"{id}\">Approve</button><button type=\"button\" class=\"cancel\" data-pid=\"{id}\">Cancel</button></div></article>",
                    esc(&name)
                )
            })
            .collect()
    };
    let log = if undo.is_empty() {
        "<p class=\"muted\">Nothing yet.</p>".to_string()
    } else {
        let li: String = undo
            .iter()
            .map(|u| {
                if u.undone {
                    format!("<li><s>{}</s> <span class=\"muted\">undone</span></li>", esc(&u.desc))
                } else {
                    format!("<li>{} <button type=\"button\" class=\"undo\" data-uid=\"{}\">Undo</button></li>", esc(&u.desc), u.id)
                }
            })
            .collect();
        format!("<ul class=\"log\">{li}</ul>")
    };
    let body = format!(
        r#"<h1>Daily planner</h1>
<p class="lead">The planner only makes suggestions. <b>Nothing changes until you press Approve.</b></p>
<section class="card"><div class="row"><button id="plan-day" type="button" class="primary">Plan my day</button><button id="plan-missed" type="button">Reschedule missed tasks</button></div>
<p class="muted">How it chooses: higher priority and nearer deadlines go first. Each task gets the earliest free gap between 9:00 AM and 6:00 PM in the next 7 days. It never lands on top of a fixed event.</p></section>
<h2>Waiting for your approval</h2>{props}
<h2>Recent changes</h2><section class="card">{log}</section>"#
    );
    page("Plan", "/plan", &body)
}

/// Plain-words observations from the numbers. Pure, so it can be tested.
pub fn insights(done_today: usize, done_week: usize, high_done_week: usize, overdue: usize, overdue_high: usize, busiest: Option<(NaiveDate, usize)>, now: NaiveDate) -> Vec<String> {
    let mut v = Vec::new();
    if done_today > 0 {
        v.push(format!("You finished {done_today} today. Nice work."));
    } else {
        v.push("Nothing finished yet today. Pick one small task to start.".into());
    }
    if done_week > 0 {
        v.push(format!("{done_week} done in the last 7 days, about {:.1} a day.", done_week as f64 / 7.0));
        if high_done_week * 2 >= done_week {
            v.push("Half or more of what you finished was high priority. Good focus.".into());
        }
    }
    if overdue_high > 0 {
        v.push(format!("{overdue_high} overdue task(s) are high priority. Start there."));
    } else if overdue > 0 {
        v.push(format!("{overdue} task(s) are overdue. Use 'Reschedule missed tasks' on the Plan page."));
    } else {
        v.push("Nothing is overdue.".into());
    }
    if let Some((d, n)) = busiest {
        if n >= 3 {
            v.push(format!("{} looks busy with {n} items. Consider moving one.", day_label(d, now)));
        }
    }
    v
}

pub fn summary_page(items: &[Item], comps_today: &[Completion], comps_week: &[Completion], now: NaiveDateTime) -> String {
    let today = now.date();
    let open: Vec<&Item> = items.iter().filter(|i| !i.done).collect();
    let overdue: Vec<&Item> = open.iter().copied().filter(|i| i.is_overdue(now)).collect();
    let due_today: Vec<&Item> = open.iter().copied().filter(|i| !i.is_overdue(now) && i.at_local().map_or(false, |a| a.date() == today)).collect();
    let upcoming: Vec<&Item> = open
        .iter()
        .copied()
        .filter(|i| i.at_local().map_or(false, |a| a.date() > today && (a.date() - today).num_days() <= 7))
        .collect();
    let mut busiest: Option<(NaiveDate, usize)> = None;
    for k in 0..7 {
        let d = today + Duration::days(k);
        let n = open.iter().filter(|i| i.at_local().map_or(false, |a| a.date() == d)).count();
        if busiest.map_or(n > 0, |b| n > b.1) {
            busiest = Some((d, n));
        }
    }
    let ins = insights(
        comps_today.len(),
        comps_week.len(),
        comps_week.iter().filter(|c| c.priority == 2).count(),
        overdue.len(),
        overdue.iter().filter(|i| i.priority == 2).count(),
        busiest,
        today,
    );
    let names = |v: &[&Item]| -> String {
        if v.is_empty() {
            "<li class=\"muted\">None</li>".into()
        } else {
            v.iter().map(|i| format!("<li>{} <span class=\"muted\">{}</span></li>", esc(&i.title), esc(&when_label(i, now)))).collect()
        }
    };
    let done_list: String = if comps_today.is_empty() {
        "<li class=\"muted\">None yet</li>".into()
    } else {
        comps_today.iter().map(|c| format!("<li>{}</li>", esc(&c.title))).collect()
    };
    let insight_li: String = ins.iter().map(|s| format!("<li>{}</li>", esc(s))).collect();
    let body = format!(
        r#"<h1>Summary</h1>
<section class="card"><h2>Insights</h2><ul>{insight_li}</ul></section>
<section class="card"><h2>Daily summary</h2>
<div class="stats"><div><b>{}</b><span>Done today</span></div><div><b>{}</b><span>Still due today</span></div><div class="{}"><b>{}</b><span>Overdue</span></div></div>
<h3>Done today</h3><ul>{done_list}</ul><h3>Still due today</h3><ul>{}</ul><h3>Overdue</h3><ul>{}</ul></section>
<section class="card"><h2>Weekly summary</h2>
<div class="stats"><div><b>{}</b><span>Done in 7 days</span></div><div><b>{}</b><span>Coming up in 7 days</span></div></div>
<h3>Coming up</h3><ul>{}</ul></section>"#,
        comps_today.len(),
        due_today.len(),
        if overdue.is_empty() { "" } else { "warn" },
        overdue.len(),
        names(&due_today),
        names(&overdue),
        comps_week.len(),
        upcoming.len(),
        names(&upcoming)
    );
    page("Summary", "/summary", &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::to_utc;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(9, 0, 0).unwrap()
    }
    fn evil(title: &str) -> Item {
        Item {
            id: 1,
            title: title.into(),
            kind: "task".into(),
            at: Some(to_utc(now())),
            has_time: true,
            dur_min: 30,
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
        }
    }
    const EVIL: &str = "<script>alert(1)</script> \"quoted\" & 'single'";

    #[test]
    fn esc_handles_all_five_characters() {
        assert_eq!(esc("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }

    fn assert_safe(html: &str) {
        assert!(!html.contains("<script>alert(1)"), "raw script tag leaked");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("title=\"<"));
    }

    #[test]
    fn user_text_is_escaped_on_every_page() {
        let items = vec![evil(EVIL)];
        assert_safe(&today_page(&items, 0, 0, now()));
        assert_safe(&calendar_page(&items, true, now().date(), now()));
        assert_safe(&calendar_page(&items, false, now().date(), now()));
        assert_safe(&summary_page(&items, &[Completion { title: EVIL.into(), priority: 1, done_at: 0 }], &[], now()));
        let ch = Change::Delete { ids: vec![1], titles: vec![EVIL.into()], events: 0, tasks: 1, recurring_series: 1 };
        assert_safe(&plan_page(&[(1, ch)], &[UndoRow { id: 1, desc: EVIL.into(), undone: false }], now()));
    }

    #[test]
    fn first_screen_is_never_blank() {
        let html = today_page(&[], 0, 0, now());
        assert!(html.contains("Load sample day"));
        assert!(html.contains("Remind me to call John tomorrow at 5 PM"));
        assert!(html.contains("You're clear"));
        assert!(html.contains("Smart text reading, works offline"));
        assert!(html.contains("day-first"));
        assert!(!html.to_lowercase().contains("ai understands"));
    }

    #[test]
    fn insights_are_plain() {
        let v = insights(0, 0, 0, 0, 0, None, now().date());
        assert!(v.iter().any(|s| s.contains("Nothing is overdue")));
        let v = insights(2, 10, 6, 3, 1, Some((now().date(), 4)), now().date());
        assert!(v.iter().any(|s| s.contains("busy")));
        assert!(v.iter().any(|s| s.contains("high priority")));
    }
}
