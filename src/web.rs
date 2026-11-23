//! HTTP routes. Bound to 127.0.0.1 only (see main.rs).
use crate::ops::{self, NewItem, Outcome};
use crate::parser;
use crate::repeat::Repeat;
use crate::store::{now_local, to_utc, Db, Undo};
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
        .route("/api/tasks", post(api_save))
        .route("/api/tasks/:id/done", post(api_done))
        .route("/api/tasks/:id/edit", post(api_edit))
        .route("/api/delete", post(api_delete))
        .route("/api/move", post(api_move))
        .route("/api/plan", post(api_plan))
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

async fn index(State(s): State<Shared>) -> Html<String> {
    let db = lock(&s);
    let now = now_local();
    let (a, b) = day_range(now.date(), 1);
    let items = db.list(false).unwrap_or_default();
    let done = db.completions_between(a, b).map(|v| v.len()).unwrap_or(0);
    let pending = db.pending_proposals().map(|v| v.len()).unwrap_or(0);
    Html(view::today_page(&items, done, pending, now))
}

async fn calendar(State(s): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Html<String> {
    let db = lock(&s);
    let now = now_local();
    let week = q.get("view").map_or(true, |v| v != "day");
    let date = q.get("date").and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).unwrap_or(now.date());
    Html(view::calendar_page(&db.list(false).unwrap_or_default(), week, date, now))
}

async fn plan_page(State(s): State<Shared>) -> Html<String> {
    let db = lock(&s);
    Html(view::plan_page(&db.pending_proposals().unwrap_or_default(), &db.recent_undo(10).unwrap_or_default(), now_local()))
}

async fn summary(State(s): State<Shared>) -> Html<String> {
    let db = lock(&s);
    let now = now_local();
    let (a, b) = day_range(now.date(), 1);
    let (w, _) = day_range(now.date() - Duration::days(6), 1);
    let today = db.completions_between(a, b).unwrap_or_default();
    let week = db.completions_between(w, b).unwrap_or_default();
    Html(view::summary_page(&db.list(false).unwrap_or_default(), &today, &week, now))
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
async fn api_parse(Json(r): Json<ParseReq>) -> Json<Value> {
    Json(parser::parse(&r.text, now_local()).to_json())
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

async fn api_save(State(s): State<Shared>, Json(r): Json<SaveReq>) -> ApiRes {
    let now = now_local();
    let date = nz(&r.date).map(parse_date).transpose()?;
    let time = match nz(&r.time) {
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
    let until = nz(&r.until).map(parse_date).transpose()?;
    let mut repeat = match nz(&r.repeat) {
        None => None,
        Some(t) if t.eq_ignore_ascii_case("none") => None,
        Some(t) => Some(parser::parse_repeat_phrase(t, now).ok_or_else(|| bad("I could not read the repeat. Try: every week on mon and wed"))?),
    };
    if let (Some(Repeat::Monthly(0)), Some(d)) = (&repeat, date) {
        use chrono::Datelike;
        repeat = Some(Repeat::Monthly(d.day()));
    }
    if repeat.is_some() && date.is_none() {
        return Err(bad("A repeat needs a start date. Pick a date too."));
    }
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
    let it = ops::create(&db, &n, false).map_err(bad)?;
    let undo = db
        .add_undo(&format!("Added: {}", it.title), &Undo { remove_items: vec![it.id], ..Undo::default() }, to_utc(now))
        .map_err(bad)?;
    Ok(Json(json!({ "status": "applied", "undo": undo, "id": it.id, "message": "Saved." })))
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
}
#[derive(Deserialize)]
struct MoveReq {
    moves: Vec<MoveItem>,
}
async fn api_move(State(s): State<Shared>, Json(r): Json<MoveReq>) -> ApiRes {
    let mut mv = Vec::new();
    for m in &r.moves {
        mv.push((m.id, parse_dt(&m.to)?));
    }
    Ok(outcome(ops::submit_move(&lock(&s), &mv, now_local()).map_err(bad)?, "Moved."))
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
