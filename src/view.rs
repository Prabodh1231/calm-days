//! HTML pages. Plain `format!` plus `esc()`. Every piece of user text goes through `esc()`.
use crate::approval::Change;
use crate::planner::NowTask;
use crate::repeat;
use crate::store::{Completion, Item, ListRow, UndoRow};
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

pub fn page(board: &str, title: &str, active: &str, body: &str) -> String {
    let nav = [("/", "Board"), ("/calendar", "Calendar"), ("/summary", "Summary")]
        .iter()
        .map(|(h, l)| {
            let cur = if *h == active { " aria-current=\"page\"" } else { "" };
            format!("<a href=\"{h}\"{cur}>{l}</a>")
        })
        .collect::<String>();
    let plan_cur = if active == "/plan" { " aria-current=\"page\"" } else { "" };
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - {board}</title>
<script>document.documentElement.dataset.theme=new URLSearchParams(location.search).get('theme')||localStorage.getItem('theme')||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');</script>
<link rel="stylesheet" href="/static/style.css">
</head><body class="{bodycls}">
<header class="top"><a class="brand" href="/" aria-label="Calm Days home">Calm Days</a>
<input id="board-name" class="board-name" type="text" maxlength="60" value="{board}" aria-label="Board name (click to rename)">
<nav aria-label="Main">{nav}</nav>
<div class="tools"><button id="now-btn" type="button" class="primary">What should I do now?</button>
<a class="btn" href="/plan"{plan_cur}>Planner</a>
<button id="theme" type="button" aria-label="Switch between light and dark mode">Dark mode</button></div></header>
<div id="banner" role="status" aria-live="polite"></div>
<main>{body}</main>
<dialog id="move-dialog"><form method="dialog"><h2>Move to...</h2>
<p id="move-title" class="muted"></p>
<label for="move-when">New date and time</label>
<input id="move-when" type="datetime-local" required>
<div class="row"><button id="move-ok" type="button" class="primary">Move</button><button id="move-cancel" type="button">Cancel</button></div>
</form></dialog>
<dialog id="cardmove-dialog"><form method="dialog"><h2>Move card to...</h2>
<p id="cm-title" class="muted"></p>
<label for="cm-list">List</label><select id="cm-list"></select>
<label for="cm-pos">Position</label><select id="cm-pos"></select>
<div class="row"><button id="cm-ok" type="button" class="primary">Move</button><button id="cm-cancel" type="button">Cancel</button></div>
</form></dialog>
<dialog id="moveall-dialog"><form method="dialog"><h2>Move all cards</h2>
<p id="ma-title" class="muted"></p>
<label for="ma-list">Move them to</label><select id="ma-list"></select>
<div class="row"><button id="ma-ok" type="button" class="primary">Move all</button><button id="ma-cancel" type="button">Cancel</button></div>
</form></dialog>
<dialog id="now-dialog"><h2>What should I do now?</h2><div id="now-body"></div>
<div class="row"><button id="now-open" type="button" class="primary" hidden>Open card</button><button id="now-done" type="button" hidden>Mark done</button><button id="now-close" type="button">Close</button></div></dialog>
<aside id="panel" aria-label="Card details" hidden></aside>
<div id="toast" role="status" aria-live="polite" hidden></div>
<script src="/static/app.js"></script>
</body></html>"##,
        title = esc(title),
        board = esc(board),
        nav = nav,
        body = body,
        plan_cur = plan_cur,
        bodycls = if active == "/" { "board-bg" } else { "" }
    )
}

/// Short due text for a card face: "Today, 5:00 PM", "Tomorrow", "Mon 30 Nov".
pub fn due_text(it: &Item, now: NaiveDateTime) -> String {
    match it.at_local() {
        None => String::new(),
        Some(a) if it.has_time => dt_label(a, now),
        Some(a) => day_label(a.date(), now.date()),
    }
}

/// Which due filter a card belongs to: done, overdue, today, week, later or none.
pub fn due_class(it: &Item, now: NaiveDateTime) -> &'static str {
    if it.done {
        return "done";
    }
    if it.is_overdue(now) {
        return "overdue";
    }
    match it.at_local().map(|a| (a.date() - now.date()).num_days()) {
        None => "none",
        Some(0) => "today",
        Some(n) if n > 0 && n <= 7 => "week",
        _ => "later",
    }
}

pub fn card_html(it: &Item, now: NaiveDateTime) -> String {
    let labels: String = it.label_keys().iter().map(|l| format!("<span class=\"lbl {l}\" title=\"{l}\"><span class=\"sr\">{l} label</span></span>")).collect();
    let lbls = if labels.is_empty() { String::new() } else { format!("<div class=\"lbls\">{labels}</div>") };
    let dc = due_class(it, now);
    let mut meta = format!("<span class=\"pri p{p}\">{}</span>", PRIO[it.priority.clamp(0, 2) as usize], p = it.priority.clamp(0, 2));
    if it.is_event() {
        meta.push_str("<span class=\"chip fixed\">Fixed event</span>");
    }
    if it.at.is_some() {
        let cls = match dc {
            "overdue" => " overdue",
            "today" => " today",
            "done" => " okdue",
            _ => "",
        };
        let word = if dc == "overdue" { "Overdue: " } else { "Due " };
        meta.push_str(&format!("<span class=\"chip due{cls}\">{}{}</span>", word, esc(&due_text(it, now))));
    }
    if !it.checklist.is_empty() {
        let n = it.checklist.iter().filter(|c| c.d).count();
        let full = if n == it.checklist.len() { " full" } else { "" };
        meta.push_str(&format!("<span class=\"chip chk{full}\" title=\"Checklist\">&#9745; {n}/{}</span>", it.checklist.len()));
    }
    if let Some(r) = &it.repeat {
        meta.push_str(&format!("<span class=\"chip icon\" title=\"Repeats: {t}\" aria-label=\"Repeats: {t}\">&#8635;</span>", t = esc(&repeat::describe(r))));
    }
    if it.remind_at.is_some() && !it.done {
        meta.push_str("<span class=\"chip icon\" title=\"Has a reminder\" aria-label=\"Has a reminder\">&#128276;</span>");
    }
    if !it.notes.trim().is_empty() {
        meta.push_str("<span class=\"chip icon\" title=\"Has notes\" aria-label=\"Has notes\">&#9776;</span>");
    }
    let q = format!("{} {}", it.title, it.notes).to_lowercase();
    format!(
        "<article class=\"bcard{done}\" draggable=\"true\" tabindex=\"0\" role=\"button\" aria-label=\"Open card: {t}\" data-id=\"{id}\" data-labels=\"{lab}\" data-due=\"{dc}\" data-q=\"{q}\">{lbls}<div class=\"ctitle\">{t}</div><div class=\"meta\">{meta}</div><button type=\"button\" class=\"cmove\" data-id=\"{id}\" data-title=\"{t}\" aria-label=\"Move {t} to another list\" title=\"Move to...\">&#8644;</button></article>",
        done = if it.done { " is-done" } else { "" },
        t = esc(&it.title),
        id = it.id,
        lab = esc(&it.labels),
        q = esc(&q),
    )
}

const LABEL_NAMES: [&str; 6] = ["green", "yellow", "orange", "red", "purple", "blue"];

pub fn board_page(board: &str, lists: &[ListRow], cards: &[Item], pending: usize, now: NaiveDateTime) -> String {
    let lists_json = serde_json::to_string(&lists.iter().map(|l| serde_json::json!({"id": l.id, "name": l.name})).collect::<Vec<_>>()).unwrap_or_default();
    let n_lists = lists.len();
    let mut cols = String::new();
    for (i, l) in lists.iter().enumerate() {
        let mine: Vec<&Item> = cards.iter().filter(|c| c.list_id == l.id).collect();
        let html: String = mine.iter().map(|c| card_html(c, now)).collect();
        let tick = if l.done_list { "<span class=\"tick\" title=\"Cards moved here count as done\" aria-label=\"Done list\">&#10003;</span>" } else { "" };
        let left = if i > 0 { format!("<button type=\"button\" class=\"l-left\" data-id=\"{}\">Move list left</button>", l.id) } else { String::new() };
        let right = if i + 1 < n_lists { format!("<button type=\"button\" class=\"l-right\" data-id=\"{}\">Move list right</button>", l.id) } else { String::new() };
        let all = if n_lists > 1 { format!("<button type=\"button\" class=\"l-moveall\" data-id=\"{}\" data-name=\"{}\">Move all cards to...</button>", l.id, esc(&l.name)) } else { String::new() };
        cols.push_str(&format!(
            r##"<section class="list" data-id="{id}" aria-label="List: {n}">
<div class="lhead"><input class="lname" type="text" maxlength="60" value="{n}" data-id="{id}" aria-label="List name"><span class="count" aria-label="{c} cards">{c}</span>{tick}
<details class="lmenu"><summary aria-label="Actions for list {n}">&#8943;</summary><div class="menu">{left}{right}{all}<button type="button" class="l-archive" data-id="{id}" data-name="{n}">Archive this list</button></div></details></div>
<div class="cards" data-list="{id}">{html}</div>
<div class="adder"><button type="button" class="add-open" data-list="{id}">+ Add a card</button>
<div class="add-form" hidden><textarea class="add-text" rows="2" maxlength="500" placeholder="Try: Remind me to call John tomorrow at 5 PM" aria-label="Describe the card in a plain sentence"></textarea>
<div class="prev" aria-live="polite"></div>
<div class="row"><button type="button" class="add-read primary">Read it</button><button type="button" class="add-save primary" hidden>Add card</button><button type="button" class="add-cancel">Cancel</button></div></div></div>
</section>"##,
            id = l.id,
            n = esc(&l.name),
            c = mine.len(),
            html = html,
            tick = tick,
            left = left,
            right = right,
            all = all,
        ));
    }
    cols.push_str("<section class=\"list addlist\"><button type=\"button\" id=\"addlist-open\">+ Add another list</button><form id=\"addlist-form\" hidden><input id=\"addlist-name\" type=\"text\" maxlength=\"60\" placeholder=\"List name\" aria-label=\"New list name\"><div class=\"row\"><button type=\"submit\" class=\"primary\">Add list</button><button type=\"button\" id=\"addlist-cancel\">Cancel</button></div></form></section>");
    let label_opts: String = LABEL_NAMES.iter().map(|l| format!("<option value=\"{l}\">{l}</option>")).collect();
    let proposals = if pending > 0 {
        format!("<p class=\"notice\"><a href=\"/plan\">{pending} suggested change(s) are waiting for your approval.</a></p>")
    } else {
        String::new()
    };
    let welcome = if cards.is_empty() {
        "<div class=\"welcome\"><b>This board is empty.</b> Add a card to any list, or <button type=\"button\" id=\"sample\">Load sample board</button></div>"
    } else {
        ""
    };
    let body = format!(
        r##"<div class="bar"><input id="q" type="search" placeholder="Search cards" aria-label="Search cards">
<select id="f-label" aria-label="Filter by label"><option value="">All labels</option>{label_opts}</select>
<select id="f-due" aria-label="Filter by due date"><option value="">Any due date</option><option value="overdue">Overdue</option><option value="today">Due today</option><option value="week">Due in 7 days</option><option value="none">No due date</option></select>
<button type="button" id="f-clear">Clear</button><span id="f-count" class="muted" aria-live="polite"></span></div>
<p class="intro">Drag cards between lists. Add a card by typing a plain sentence. Smart text reading, works offline: it follows simple rules and is not an AI, so check the preview. Numeric dates are day-first: 12/11 means 12 November.</p>
{proposals}{welcome}
<div class="board" id="board" data-lists="{lj}">{cols}</div>"##,
        lj = esc(&lists_json),
    );
    page(board, "Board", "/", &body)
}

fn item_row(it: &Item, now: NaiveDateTime) -> String {
    let at = it.at_local().map(|a| a.format("%Y-%m-%dT%H:%M").to_string()).unwrap_or_default();
    let t = esc(&it.title);
    let overdue = if it.is_overdue(now) { " overdue" } else { "" };
    format!(
        r#"<li class="task{overdue}" draggable="true" data-id="{id}"><div class="main"><button type="button" class="open linklike" data-id="{id}">{t}</button><span class="when">{when}</span></div>
<div class="acts"><button type="button" class="move" data-id="{id}" data-at="{at}" data-title="{t}" aria-label="Move {t} to another time">Move to...</button></div></li>"#,
        id = it.id,
        when = esc(&when_label(it, now)),
    )
}

pub fn now_task_list(items: &[Item]) -> Vec<NowTask> {
    items
        .iter()
        .filter(|i| !i.is_event() && !i.done)
        .map(|i| NowTask { id: i.id, at: i.at_local(), has_time: i.has_time, dur: i.dur_min, priority: i.priority })
        .collect()
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub fn calendar_page(board: &str, items: &[Item], week: bool, date: NaiveDate, now: NaiveDateTime) -> String {
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
                        "<div class=\"card-ev{ev}\" draggable=\"true\" data-id=\"{id}\"><b>{t}</b> <button type=\"button\" class=\"open linklike\" data-id=\"{id}\">{title}</button><button type=\"button\" class=\"move small\" data-id=\"{id}\" data-at=\"{at}\" data-title=\"{title}\" aria-label=\"Move {title} to another time\">Move to...</button></div>",
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
    page(board, "Calendar", "/calendar", &body)
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
        Change::MoveCards { moves } => (
            "Move cards".into(),
            moves.iter().map(|m| format!("Move \"{}\" from {} to {}", m.title, m.from_list, m.to_list)).collect(),
        ),
        Change::ArchiveList { name, cards, .. } => ("Archive list".into(), vec![format!("Archive the list \"{name}\" and its {cards} card(s)")]),
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

pub fn plan_page(board: &str, pending: &[(i64, Change)], undo: &[UndoRow], now: NaiveDateTime) -> String {
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
    page(board, "Planner", "/plan", &body)
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

pub fn summary_page(board: &str, items: &[Item], comps_today: &[Completion], comps_week: &[Completion], now: NaiveDateTime) -> String {
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
<h3>Coming up</h3><ul>{}</ul></section>
<section class="card"><h2>About reminders</h2><p class="muted">Reminders pop up while this app is open in a browser tab. Reminders that came due while it was closed show in a yellow bar when you return. They do not appear if the app or the browser is closed.</p>
<button id="notify" type="button">Turn on pop-up reminders</button></section>"#,
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
    page(board, "Summary", "/summary", &body)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{to_utc, Check};

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
            list_id: 1,
            pos: 0,
            notes: title.into(),
            labels: "red,<b>".into(),
            checklist: vec![Check { t: title.into(), d: true }],
            archived: false,
        }
    }
    fn lists(name: &str) -> Vec<ListRow> {
        vec![ListRow { id: 1, name: name.into(), pos: 0, archived: false, done_list: false }, ListRow { id: 2, name: "Done".into(), pos: 1, archived: false, done_list: true }]
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
        assert!(!html.contains("<b>\""), "label text leaked");
    }

    #[test]
    fn user_text_is_escaped_on_every_page() {
        let items = vec![evil(EVIL)];
        assert_safe(&board_page(EVIL, &lists(EVIL), &items, 0, now()));
        assert_safe(&calendar_page(EVIL, &items, true, now().date(), now()));
        assert_safe(&calendar_page(EVIL, &items, false, now().date(), now()));
        assert_safe(&summary_page(EVIL, &items, &[Completion { title: EVIL.into(), priority: 1, done_at: 0 }], &[], now()));
        let ch = Change::Delete { ids: vec![1], titles: vec![EVIL.into()], events: 0, tasks: 1, recurring_series: 1 };
        assert_safe(&plan_page(EVIL, &[(1, ch)], &[UndoRow { id: 1, desc: EVIL.into(), undone: false }], now()));
        let am = Change::ArchiveList { id: 1, name: EVIL.into(), cards: 2 };
        assert_safe(&plan_page("x", &[(1, am)], &[], now()));
    }

    #[test]
    fn empty_board_is_not_blank() {
        let html = board_page("My board", &lists("To do"), &[], 0, now());
        assert!(html.contains("Load sample board"));
        assert!(html.contains("+ Add a card"));
        assert!(html.contains("Remind me to call John tomorrow at 5 PM"));
        assert!(html.contains("Smart text reading, works offline"));
        assert!(html.contains("day-first"));
        assert!(html.contains("+ Add another list"));
        assert!(!html.to_lowercase().contains("ai understands"));
    }

    #[test]
    fn card_face_shows_chips() {
        let mut c = evil("Pay rent");
        c.at = Some(to_utc(now() - Duration::days(1)));
        c.has_time = false;
        c.repeat = Some(crate::repeat::Repeat::Daily(1));
        c.remind_at = Some(5);
        c.checklist = vec![Check { t: "a".into(), d: true }, Check { t: "b".into(), d: false }];
        let h = card_html(&c, now());
        assert!(h.contains("chip due overdue"), "{h}");
        assert!(h.contains("1/2"));
        assert!(h.contains("&#8635;"));
        assert!(h.contains("&#128276;"));
        assert!(h.contains("data-due=\"overdue\""));
        c.at = Some(to_utc(now()));
        assert!(card_html(&c, now()).contains("chip due today"));
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
