//! HTTP routes. Bound to 127.0.0.1 only (see main.rs).
use crate::ai;
use crate::extra;
use crate::ops::{self, NewItem, Outcome};
use crate::parser;
use crate::repeat::Repeat;
use crate::store::{now_local, to_utc, Check, Db, Item, Undo};
use crate::view;
use axum::{
    extract::{Path, Query, Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

pub const PORT: u16 = 3000;
pub type Shared = Arc<Mutex<Db>>;
type ApiRes = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn bad(msg: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg.to_string() })))
}

fn lock(s: &Shared) -> MutexGuard<'_, Db> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reject requests whose Host is not our own address (DNS rebinding) and POSTs that are not JSON (cross-site forms).
async fn guard(req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    if host != format!("127.0.0.1:{PORT}") && host != format!("localhost:{PORT}") {
        return (StatusCode::FORBIDDEN, "Open this app at http://127.0.0.1:3000").into_response();
    }
    if req.method() == Method::POST {
        let ct = req.headers().get(header::CONTENT_TYPE).and_then(|h| h.to_str().ok()).unwrap_or("");
        if !ct.starts_with("application/json") {
            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON only").into_response();
        }
    }
    next.run(req).await
}

pub fn router(shared: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/calendar", get(calendar))
        .route("/plan", get(plan_page))
        .route("/summary", get(summary))
        .route("/static/app.js", get(js))
        .route("/static/style.css", get(css))
        .route("/api/parse", post(api_parse))
        .route("/api/ai", get(api_ai_get).post(api_ai_set))
        .route("/api/ai/check", post(api_ai_check))
        .route("/api/ai/warm", post(api_ai_warm))
        .route("/api/tasks", post(api_save))
        .route("/api/now", get(api_now))
        .route("/api/cards/move", post(api_card_move))
        .route("/api/cards/index", get(api_card_index))
        .route("/api/cards/:id/resize", post(api_card_resize))
        .route("/api/cards/:id", get(api_card_get))
        .route("/api/cards/:id/update", post(api_card_update))
        .route("/api/cards/:id/archive", post(api_card_archive))
        .route("/api/lists", post(api_list_add))
        .route("/api/lists/move-all", post(api_list_move_all))
        .route("/api/lists/:id/rename", post(api_list_rename))
        .route("/api/lists/:id/move", post(api_list_move))
        .route("/api/lists/:id/archive", post(api_list_archive))
        .route("/api/board/rename", post(api_board_rename))
        .route("/api/tasks/:id/done", post(api_done))
        .route("/api/tasks/:id/edit", post(api_edit))
        .route("/api/delete", post(api_delete))
        .route("/api/move", post(api_move))
        .route("/api/plan", post(api_plan))
        .route("/api/proposals/:id", get(api_proposal))
        .route("/api/proposals/:id/approve", post(api_approve))
        .route("/api/proposals/:id/cancel", post(api_cancel))
        .route("/api/undo", post(api_undo))
        .route("/api/sample", post(api_sample))
        .route("/api/due", get(api_due))
        .route("/api/due/ack", post(api_ack))
        .layer(middleware::from_fn(guard))
        .with_state(shared)
}

/// Every 30 seconds, mark reminders that have come due. The page picks them up from /api/due.
pub fn spawn_scheduler(shared: Shared) {
    std::thread::spawn(move || loop {
        let _ = lock(&shared).check_due(to_utc(now_local()));
        std::thread::sleep(std::time::Duration::from_secs(30));
    });
}

fn day_range(d: NaiveDate, days: i64) -> (i64, i64) {
    let a = d.and_hms_opt(0, 0, 0).unwrap_or_default();
    (to_utc(a), to_utc(a + Duration::days(days)))
}

pub fn board_name(db: &Db) -> String {
    db.meta("board_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "My board".into())
}

async fn index(State(s): State<Shared>) -> Html<String> {
    let db = lock(&s);
    let now = now_local();
    let lists = db.lists(false).unwrap_or_default();
    let cards: Vec<Item> = lists.iter().flat_map(|l| db.cards_in(l.id).unwrap_or_default()).collect();
    let pending = ops::waiting_plans(&db).len();
    Html(view::board_page(&board_name(&db), &lists, &cards, pending, now))
}

async fn calendar(State(s): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Html<String> {
    let db = lock(&s);
    let now = now_local();
    let view = q.get("view").map(String::as_str).unwrap_or("week");
    let date = q.get("date").and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).unwrap_or(now.date());
    Html(crate::calview::calendar_page(&board_name(&db), &db.list(false).unwrap_or_default(), view, date, now))
}

async fn plan_page(State(s): State<Shared>) -> Html<String> {
    let db = lock(&s);
    Html(view::plan_page(&board_name(&db), &ops::waiting_plans(&db), &db.recent_undo(10).unwrap_or_default(), now_local()))
}

async fn summary(State(s): State<Shared>) -> Html<String> {
    let db = lock(&s);
    let now = now_local();
    let (a, b) = day_range(now.date(), 1);
    let (w, _) = day_range(now.date() - Duration::days(6), 1);
    let today = db.completions_between(a, b).unwrap_or_default();
    let week = db.completions_between(w, b).unwrap_or_default();
    // "Not started" for the Pulse: the Inbox and the first open list (To do).
    let lists = db.lists(false).unwrap_or_default();
    let inbox = db.inbox_id();
    let mut not_started: Vec<i64> = inbox.into_iter().collect();
    not_started.extend(lists.iter().filter(|l| !l.done_list && Some(l.id) != inbox).map(|l| l.id).take(1));
    Html(view::summary_page_with(&board_name(&db), &db.list(true).unwrap_or_default(), &today, &week, &not_started, now))
}
async fn js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("../assets/app.js"))
}
async fn css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], include_str!("../assets/style.css"))
}

#[derive(Deserialize)]
struct ParseReq {
    text: String,
}
/// Read one sentence. The model call runs on a blocking thread and holds no lock on the database.
async fn api_parse(State(s): State<Shared>, Json(r): Json<ParseReq>) -> Json<Value> {
    let st = ai::load_settings(&lock(&s));
    let text: String = r.text.chars().take(parser::MAX_INPUT_CHARS).collect();
    let out = tokio::task::spawn_blocking(move || {
        let model = if st.on { ai::Ollama::new(&st) } else { None };
        ai::read(model.as_ref().map(|m| m as &dyn ai::Reader), &text, now_local()).to_json()
    })
    .await;
    Json(out.unwrap_or_else(|_| json!({ "error": "Could not read that sentence." })))
}

fn settings_json(st: &ai::Settings) -> Value {
    json!({ "on": st.on, "model": st.model, "url": st.url, "default_model": ai::DEFAULT_MODEL, "default_url": ai::DEFAULT_URL })
}
async fn api_ai_get(State(s): State<Shared>) -> Json<Value> {
    Json(settings_json(&ai::load_settings(&lock(&s))))
}
#[derive(Deserialize)]
struct AiReq {
    on: bool,
    model: String,
    url: String,
}
async fn api_ai_set(State(s): State<Shared>, Json(r): Json<AiReq>) -> ApiRes {
    let model = r.model.trim();
    let url = r.url.trim().trim_end_matches('/');
    if !ai::valid_model(model) {
        return Err(bad("The model name needs letters, numbers and . : _ - / only (80 at most)."));
    }
    if ai::local_host_port(url).is_none() {
        return Err(bad("The address must be on this computer: http://127.0.0.1:11434 or http://localhost:11434"));
    }
    let db = lock(&s);
    db.set_meta("ai_on", if r.on { "1" } else { "0" }).map_err(bad)?;
    db.set_meta("ai_model", model).map_err(bad)?;
    db.set_meta("ai_url", url).map_err(bad)?;
    Ok(Json(json!({ "status": "ok", "message": "AI settings saved.", "settings": settings_json(&ai::load_settings(&db)) })))
}
/// Is Ollama running, and is the model installed?
async fn api_ai_check(State(s): State<Shared>) -> Json<Value> {
    let st = ai::load_settings(&lock(&s));
    let (running, has) = tokio::task::spawn_blocking(move || ai::check(&st)).await.unwrap_or((false, false));
    Json(json!({ "running": running, "model_installed": has }))
}
/// Load the model into memory when the user starts typing a card. Answers at once.
async fn api_ai_warm(State(s): State<Shared>) -> Json<Value> {
    let st = ai::load_settings(&lock(&s));
    if st.on {
        std::thread::spawn(move || ai::warm(&st));
    }
    Json(json!({ "status": "ok" }))
}

#[derive(Deserialize)]
struct SaveReq {
    title: String,
    date: Option<String>,
    time: Option<String>,
    priority: Option<i64>,
    reminder_min: Option<i64>,
    repeat: Option<String>,
    until: Option<String>,
    dur_min: Option<i64>,
    kind: Option<String>,
    list_id: Option<i64>,
    /// Colour keys from the reader. Unknown ones are ignored.
    labels: Option<Vec<String>>,
}

fn nz(o: &Option<String>) -> Option<&str> {
    o.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn parse_date(s: &str) -> Result<NaiveDate, (StatusCode, Json<Value>)> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| bad("The date looks wrong. Use the date box."))
}

fn parse_dt(s: &str) -> Result<NaiveDateTime, (StatusCode, Json<Value>)> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|_| bad("The date and time look wrong."))
}

type When = (Option<NaiveDate>, Option<NaiveTime>, Option<Repeat>, Option<NaiveDate>);

/// Read the date, time, repeat and until boxes of a form.
fn read_when(date: &Option<String>, time: &Option<String>, repeat: &Option<String>, until: &Option<String>, now: NaiveDateTime) -> Result<When, (StatusCode, Json<Value>)> {
    let date = nz(date).map(parse_date).transpose()?;
    let time = match nz(time) {
        Some(t) => Some(
            NaiveTime::parse_from_str(t, "%H:%M")
                .or_else(|_| NaiveTime::parse_from_str(t, "%H:%M:%S"))
                .map_err(|_| bad("The time looks wrong. Use the time box."))?,
        ),
        None => None,
    };
    if time.is_some() && date.is_none() {
        return Err(bad("A time needs a date. Pick a date too."));
    }
    let until = nz(until).map(parse_date).transpose()?;
    let mut rep = match nz(repeat) {
        None => None,
        Some(t) if t.eq_ignore_ascii_case("none") => None,
        Some(t) => Some(parser::parse_repeat_phrase(t, now).ok_or_else(|| bad("I could not read the repeat. Try: every week on mon and wed"))?),
    };
    if let (Some(Repeat::Monthly(0)), Some(d)) = (&rep, date) {
        use chrono::Datelike;
        rep = Some(Repeat::Monthly(d.day()));
    }
    if rep.is_some() && date.is_none() {
        return Err(bad("A repeat needs a start date. Pick a date too."));
    }
    Ok((date, time, rep, until))
}

async fn api_save(State(s): State<Shared>, Json(r): Json<SaveReq>) -> ApiRes {
    let now = now_local();
    let (date, time, repeat, until) = read_when(&r.date, &r.time, &r.repeat, &r.until, now)?;
    let n = NewItem {
        title: r.title,
        is_event: r.kind.as_deref() == Some("event"),
        date,
        time,
        dur_min: r.dur_min.unwrap_or(30),
        priority: r.priority.unwrap_or(1),
        reminder_min: r.reminder_min,
        repeat,
        until,
    };
    let db = lock(&s);
    let list = match r.list_id {
        Some(l) => l,
        None => ops::capture_list(&db).map_err(bad)?,
    };
    let mut it = ops::create_in(&db, &n, false, list).map_err(bad)?;
    let mut keys: Vec<&str> = Vec::new();
    for l in r.labels.iter().flatten() {
        if let Some(k) = crate::store::LABELS.iter().find(|k| **k == l.as_str()) {
            if !keys.contains(k) {
                keys.push(k);
            }
        }
    }
    if !keys.is_empty() {
        it.labels = keys.join(",");
        db.put(&it).map_err(bad)?;
    }
    let undo = db
        .add_undo(&format!("Added: {}", it.title), &Undo { remove_items: vec![it.id], ..Undo::default() }, to_utc(now))
        .map_err(bad)?;
    Ok(Json(json!({ "status": "applied", "undo": undo, "id": it.id, "message": "Saved." })))
}

// ---- cards ----
async fn api_card_get(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    let db = lock(&s);
    let it = db.get(id).map_err(bad)?.ok_or_else(|| bad("That card no longer exists."))?;
    let now = now_local();
    let index = db.cards_in(it.list_id).map_err(bad)?.iter().position(|c| c.id == id).unwrap_or(0);
    let a = it.at_local();
    Ok(Json(json!({
        "id": it.id, "title": it.title, "notes": it.notes, "priority": it.priority,
        "labels": it.label_keys(), "checklist": it.checklist,
        "date": a.map(|a| a.format("%Y-%m-%d").to_string()),
        "time": a.filter(|_| it.has_time).map(|a| a.format("%H:%M").to_string()),
        "reminder_min": it.reminder_min,
        "repeat": it.repeat.as_ref().map(crate::repeat::describe).unwrap_or_default(),
        "until": it.until.map(|d| d.format("%Y-%m-%d").to_string()),
        "kind": it.kind, "dur_min": it.dur_min, "list_id": it.list_id, "index": index, "done": it.done,
        "cid": extra::short_id(it.id),
        "cycle": extra::cycle_choice(&it.cycle, now.date()),
        "suggest": extra::suggest_labels(&it.title, &it.notes, &it.label_keys().iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        "activity": db.activity_for(id, 30).unwrap_or_default().iter().map(|(t, at)| json!({ "text": t, "ago": extra::ago(to_utc(now), *at) })).collect::<Vec<_>>(),
    })))
}

/// Every open card with its short id, for the command menu.
async fn api_card_index(State(s): State<Shared>) -> ApiRes {
    let db = lock(&s);
    let lists = db.lists(false).map_err(bad)?;
    let mut out = Vec::new();
    for l in &lists {
        for c in db.cards_in(l.id).map_err(bad)? {
            out.push(json!({ "id": c.id, "cid": extra::short_id(c.id), "title": c.title, "list": l.name, "list_id": l.id, "done": c.done }));
        }
    }
    Ok(Json(json!({ "cards": out, "lists": lists.iter().map(|l| json!({ "id": l.id, "name": l.name })).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct ResizeReq {
    dur_min: i64,
}
async fn api_card_resize(State(s): State<Shared>, Path(id): Path<i64>, Json(r): Json<ResizeReq>) -> ApiRes {
    let u = ops::resize_card(&lock(&s), id, r.dur_min, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "Length changed."))
}

#[derive(Deserialize)]
struct CardReq {
    title: String,
    notes: Option<String>,
    priority: Option<i64>,
    labels: Option<Vec<String>>,
    checklist: Option<Vec<Check>>,
    date: Option<String>,
    time: Option<String>,
    reminder_min: Option<i64>,
    repeat: Option<String>,
    until: Option<String>,
    kind: Option<String>,
    dur_min: Option<i64>,
    cycle: Option<String>,
}
async fn api_card_update(State(s): State<Shared>, Path(id): Path<i64>, Json(r): Json<CardReq>) -> ApiRes {
    let now = now_local();
    let (date, time, repeat, until) = read_when(&r.date, &r.time, &r.repeat, &r.until, now)?;
    let f = ops::CardForm {
        title: r.title,
        notes: r.notes.unwrap_or_default(),
        priority: r.priority.unwrap_or(1),
        labels: r.labels.unwrap_or_default(),
        checklist: r.checklist.unwrap_or_default(),
        date,
        time,
        reminder_min: r.reminder_min,
        repeat,
        until,
        is_event: r.kind.as_deref() == Some("event"),
        dur_min: r.dur_min.unwrap_or(30),
    };
    let u = ops::update_card_ext(&lock(&s), id, &f, r.cycle.as_deref(), now).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "Saved."))
}
async fn api_card_archive(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    let u = ops::archive_card(&lock(&s), id, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "Card archived."))
}

#[derive(Deserialize)]
struct CardMoveItem {
    id: i64,
    list: i64,
    index: Option<usize>,
}
#[derive(Deserialize)]
struct CardMoveReq {
    moves: Vec<CardMoveItem>,
}
async fn api_card_move(State(s): State<Shared>, Json(r): Json<CardMoveReq>) -> ApiRes {
    let m: Vec<(i64, i64, Option<usize>)> = r.moves.iter().map(|m| (m.id, m.list, m.index)).collect();
    Ok(outcome(ops::submit_card_moves(&lock(&s), &m, now_local()).map_err(bad)?, "Moved."))
}

// ---- lists ----
#[derive(Deserialize)]
struct NameReq {
    name: String,
}
async fn api_list_add(State(s): State<Shared>, Json(r): Json<NameReq>) -> ApiRes {
    let u = ops::add_list(&lock(&s), &r.name, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "List added."))
}
async fn api_list_rename(State(s): State<Shared>, Path(id): Path<i64>, Json(r): Json<NameReq>) -> ApiRes {
    let u = ops::rename_list(&lock(&s), id, &r.name, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "List renamed."))
}
#[derive(Deserialize)]
struct IndexReq {
    index: usize,
}
async fn api_list_move(State(s): State<Shared>, Path(id): Path<i64>, Json(r): Json<IndexReq>) -> ApiRes {
    let u = ops::move_list(&lock(&s), id, r.index, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "List moved."))
}
async fn api_list_archive(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    Ok(outcome(ops::submit_archive_list(&lock(&s), id, now_local()).map_err(bad)?, "List archived."))
}
#[derive(Deserialize)]
struct MoveAllReq {
    from: i64,
    to: i64,
}
async fn api_list_move_all(State(s): State<Shared>, Json(r): Json<MoveAllReq>) -> ApiRes {
    Ok(outcome(ops::submit_move_all(&lock(&s), r.from, r.to, now_local()).map_err(bad)?, "Cards moved."))
}
async fn api_board_rename(State(s): State<Shared>, Json(r): Json<NameReq>) -> ApiRes {
    let n = r.name.trim();
    if n.is_empty() || n.chars().count() > 60 {
        return Err(bad("The board name needs 1 to 60 letters."));
    }
    lock(&s).set_meta("board_name", n).map_err(bad)?;
    Ok(Json(json!({ "status": "ok", "message": "Board renamed." })))
}

/// "What should I do now?" for the top bar.
async fn api_now(State(s): State<Shared>) -> ApiRes {
    let db = lock(&s);
    let now = now_local();
    let items = db.list(false).map_err(bad)?;
    Ok(Json(match crate::planner::do_now(now, &view::now_task_list(&items)) {
        Some((id, why)) => match items.iter().find(|i| i.id == id) {
            Some(it) => json!({ "id": id, "title": it.title, "why": why, "when": view::when_label(it, now) }),
            None => json!({ "clear": true }),
        },
        None => json!({ "clear": true }),
    }))
}
fn outcome(o: Outcome, msg: &str) -> Json<Value> {
    match o {
        Outcome::Applied(u) => Json(json!({ "status": "applied", "undo": u, "message": msg })),
        Outcome::Proposal(p) => Json(json!({ "status": "proposal", "proposal": p, "message": "This needs your approval first. Nothing was changed." })),
    }
}

async fn api_done(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    let u = ops::complete(&lock(&s), id, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "Marked done."))
}

#[derive(Deserialize)]
struct EditReq {
    title: Option<String>,
    priority: Option<i64>,
}
async fn api_edit(State(s): State<Shared>, Path(id): Path<i64>, Json(r): Json<EditReq>) -> ApiRes {
    Ok(outcome(ops::submit_edit(&lock(&s), id, r.title, r.priority, now_local()).map_err(bad)?, "Saved."))
}

#[derive(Deserialize)]
struct DeleteReq {
    ids: Vec<i64>,
}
async fn api_delete(State(s): State<Shared>, Json(r): Json<DeleteReq>) -> ApiRes {
    Ok(outcome(ops::submit_delete(&lock(&s), &r.ids, now_local()).map_err(bad)?, "Deleted."))
}

#[derive(Deserialize)]
struct MoveItem {
    id: i64,
    to: String,
    allday: Option<bool>,
}
#[derive(Deserialize)]
struct MoveReq {
    moves: Vec<MoveItem>,
}
async fn api_move(State(s): State<Shared>, Json(r): Json<MoveReq>) -> ApiRes {
    let mut mv = Vec::new();
    for m in &r.moves {
        mv.push((m.id, parse_dt(&m.to)?, m.allday.unwrap_or(false)));
    }
    Ok(outcome(ops::submit_move_ex(&lock(&s), &mv, now_local()).map_err(bad)?, "Moved."))
}

#[derive(Deserialize)]
struct PlanReq {
    missed: Option<bool>,
}
async fn api_plan(State(s): State<Shared>, Json(r): Json<PlanReq>) -> ApiRes {
    match ops::make_plan(&lock(&s), now_local(), r.missed.unwrap_or(false)).map_err(bad)? {
        Some(p) => Ok(Json(json!({ "status": "proposal", "proposal": p, "message": "Here is a suggestion. Nothing changes until you approve." }))),
        None => Ok(Json(json!({ "status": "none", "message": "Nothing needs a new time right now." }))),
    }
}

/// The plain-words question for the approval dialog on the board.
async fn api_proposal(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    let (c, status) = lock(&s).get_proposal(id).map_err(bad)?.ok_or_else(|| bad("That suggestion no longer exists."))?;
    if status != "pending" {
        return Err(bad("That suggestion was already answered."));
    }
    Ok(Json(view::confirm_json(&c, now_local())))
}
async fn api_approve(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    let u = ops::approve(&lock(&s), id, now_local()).map_err(bad)?;
    Ok(outcome(Outcome::Applied(u), "Approved and applied."))
}
async fn api_cancel(State(s): State<Shared>, Path(id): Path<i64>) -> ApiRes {
    ops::cancel(&lock(&s), id).map_err(bad)?;
    Ok(Json(json!({ "status": "cancelled", "message": "Cancelled. Nothing was changed." })))
}

#[derive(Deserialize)]
struct UndoReq {
    id: i64,
}
async fn api_undo(State(s): State<Shared>, Json(r): Json<UndoReq>) -> ApiRes {
    ops::undo(&lock(&s), r.id).map_err(bad)?;
    Ok(Json(json!({ "status": "undone", "message": "Undone." })))
}

async fn api_sample(State(s): State<Shared>) -> ApiRes {
    let n = ops::load_sample(&lock(&s), now_local()).map_err(bad)?;
    Ok(Json(json!({ "status": "applied", "message": format!("Added {n} example items.") })))
}

async fn api_due(State(s): State<Shared>) -> ApiRes {
    let db = lock(&s);
    let now = now_local();
    db.check_due(to_utc(now)).map_err(bad)?;
    let items: Vec<Value> = db
        .due_list()
        .map_err(bad)?
        .iter()
        .map(|i| json!({ "id": i.id, "title": i.title, "when": view::when_label(i, now) }))
        .collect();
    Ok(Json(json!({ "due": items })))
}

#[derive(Deserialize)]
struct AckReq {
    ids: Vec<i64>,
}
async fn api_ack(State(s): State<Shared>, Json(r): Json<AckReq>) -> ApiRes {
    lock(&s).ack(&r.ids).map_err(bad)?;
    Ok(Json(json!({ "status": "ok" })))
}
