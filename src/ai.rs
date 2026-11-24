//! The optional local AI sentence reader (Ollama). The model only READS a sentence and suggests card fields.
//! It never touches existing cards or the calendar. Rust checks every field; a bad field falls back to the
//! rule parser (`parser.rs`) for that field. Without Ollama, with a missing model or after the time limit, the
//! rule parser answers alone.
//!
//! HTTP: plain `std::net::TcpStream` to 127.0.0.1 / localhost only. No new crate, no TLS needed, and one
//! deadline covers connect, write and read. shortcut: HTTP/1.1 with `Connection: close`, no redirects, no
//! keep-alive; upgrade to a client crate if the model host ever moves off this computer.
use crate::extra::LABEL_TITLES;
use crate::parser::{self, Parsed, MAX_TITLE_CHARS};
use crate::repeat;
use chrono::{Duration, Timelike, NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration as StdDuration, Instant};

pub const DEFAULT_MODEL: &str = "qwen3.5:0.8b";
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";
/// How long a sentence may wait for the model before the rules answer.
pub const ANSWER_SECS: u64 = 4;
const KEEP_ALIVE: &str = "5m";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub on: bool,
    pub model: String,
    pub url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { on: true, model: DEFAULT_MODEL.into(), url: DEFAULT_URL.into() }
    }
}

/// Only this computer: http://127.0.0.1:PORT or http://localhost:PORT. Returns "host:port".
pub fn local_host_port(url: &str) -> Option<String> {
    let rest = url.trim().trim_end_matches('/').strip_prefix("http://")?;
    let (host, port) = rest.split_once(':')?;
    let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
    (host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost")).then(|| format!("{host}:{port}"))
}

pub fn valid_model(m: &str) -> bool {
    !m.is_empty() && m.len() <= 80 && m.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '_' | '-' | '/'))
}

pub fn load_settings(db: &crate::store::Db) -> Settings {
    let d = Settings::default();
    Settings {
        on: db.meta("ai_on").map(|v| v != "0").unwrap_or(d.on),
        model: db.meta("ai_model").filter(|m| valid_model(m)).unwrap_or(d.model),
        url: db.meta("ai_url").filter(|u| local_host_port(u).is_some()).unwrap_or(d.url),
    }
}

#[derive(Debug, PartialEq)]
pub enum ReadErr {
    /// Ollama is not running (connection refused or no route)
    Down,
    /// No complete answer before the deadline
    Timeout,
    /// Ollama answered with an error, for example the model is not installed
    Model(String),
    /// The answer could not be understood
    Junk,
}

impl ReadErr {
    pub fn why(&self) -> &'static str {
        match self {
            ReadErr::Down => "Ollama is not running",
            ReadErr::Timeout => "the model took too long",
            ReadErr::Model(_) => "the model is not installed",
            ReadErr::Junk => "the model gave an answer I could not use",
        }
    }
}

/// The model call, behind a trait so tests can use a fake.
pub trait Reader {
    /// Returns the model's JSON text for the sentence.
    fn ask(&self, system: &str, user: &str, schema: &Value) -> Result<String, ReadErr>;
}

pub struct Ollama {
    pub model: String,
    pub host_port: String,
    pub secs: u64,
}

impl Ollama {
    pub fn new(s: &Settings) -> Option<Ollama> {
        Some(Ollama { model: s.model.clone(), host_port: local_host_port(&s.url)?, secs: ANSWER_SECS })
    }
}

impl Reader for Ollama {
    fn ask(&self, system: &str, user: &str, schema: &Value) -> Result<String, ReadErr> {
        let deadline = Instant::now() + StdDuration::from_secs(self.secs);
        let mut body = json!({
            "model": self.model, "stream": false, "think": false, "format": schema, "keep_alive": KEEP_ALIVE,
            "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }],
            "options": { "temperature": 0, "num_predict": 300, "num_ctx": 2048 }
        });
        let mut res = http(&self.host_port, "POST", "/api/chat", Some(&body.to_string()), deadline)?;
        // A model that cannot think rejects the flag: ask again without it.
        if res.0 == 400 && res.1.contains("think") {
            body.as_object_mut().map(|o| o.remove("think"));
            res = http(&self.host_port, "POST", "/api/chat", Some(&body.to_string()), deadline)?;
        }
        let (code, text) = res;
        let v: Value = serde_json::from_str(&text).map_err(|_| ReadErr::Junk)?;
        if code != 200 {
            return Err(ReadErr::Model(v["error"].as_str().unwrap_or("error").chars().take(200).collect()));
        }
        v["message"]["content"].as_str().map(str::to_string).ok_or(ReadErr::Junk)
    }
}

/// What the settings dialog's Check button shows: (ollama running, model installed).
pub fn check(s: &Settings) -> (bool, bool) {
    let Some(hp) = local_host_port(&s.url) else { return (false, false) };
    match http(&hp, "GET", "/api/tags", None, Instant::now() + StdDuration::from_secs(2)) {
        Ok((200, t)) => {
            let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
            let want = s.model.to_lowercase();
            let has = v["models"].as_array().map(|m| m.iter().any(|x| {
                let n = x["name"].as_str().unwrap_or("").to_lowercase();
                n == want || n == format!("{want}:latest")
            })).unwrap_or(false);
            (true, has)
        }
        Ok(_) => (true, false),
        Err(_) => (false, false),
    }
}

/// Make sure Ollama is running: if not, start `ollama serve` hidden and wait up to `wait_secs`.
/// Only for the default local address. Returns (running, model installed).
pub fn ensure_running(s: &Settings, wait_secs: u64) -> (bool, bool) {
    let first = check(s);
    if first.0 || local_host_port(&s.url).as_deref() != local_host_port(DEFAULT_URL).as_deref() {
        return first;
    }
    let mut cmd = std::process::Command::new("ollama");
    cmd.arg("serve").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | 0x0000_0008); // no window, detached
    }
    if cmd.spawn().is_err() {
        return first; // Ollama is not installed
    }
    let end = Instant::now() + StdDuration::from_secs(wait_secs);
    while Instant::now() < end {
        std::thread::sleep(StdDuration::from_millis(500));
        let r = check(s);
        if r.0 {
            return r;
        }
    }
    check(s)
}

/// Load the model into memory ahead of the first sentence (empty chat, no answer). Best effort.
pub fn warm(s: &Settings) {
    if let Some(hp) = local_host_port(&s.url) {
        if !ensure_running(s, 15).0 {
            return;
        }
        let b = json!({ "model": s.model, "messages": [], "stream": false, "keep_alive": KEEP_ALIVE, "options": { "num_ctx": 2048 } }).to_string();
        let _ = http(&hp, "POST", "/api/chat", Some(&b), Instant::now() + StdDuration::from_secs(60));
    }
}

fn left(deadline: Instant) -> Result<StdDuration, ReadErr> {
    deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()).ok_or(ReadErr::Timeout)
}

/// One HTTP/1.1 request to a local server. One deadline for everything.
fn http(host_port: &str, method: &str, path: &str, body: Option<&str>, deadline: Instant) -> Result<(u16, String), ReadErr> {
    let addrs: Vec<_> = host_port.to_socket_addrs().map_err(|_| ReadErr::Down)?.collect();
    let mut stream = None;
    for a in addrs {
        if let Ok(s) = TcpStream::connect_timeout(&a, left(deadline)?) {
            stream = Some(s);
            break;
        }
    }
    let mut s = stream.ok_or(ReadErr::Down)?;
    let b = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
        b.len()
    );
    s.set_write_timeout(Some(left(deadline)?)).ok();
    s.write_all(req.as_bytes()).map_err(|_| ReadErr::Down)?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        s.set_read_timeout(Some(left(deadline)?)).ok();
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > 2_000_000 {
                    return Err(ReadErr::Junk);
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Err(ReadErr::Timeout),
            Err(_) => return Err(ReadErr::Down),
        }
    }
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").ok_or(ReadErr::Junk)?;
    let code: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or(ReadErr::Junk)?;
    let chunked = head.to_lowercase().contains("transfer-encoding: chunked");
    Ok((code, if chunked { unchunk(rest) } else { rest.to_string() }))
}

fn unchunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, after)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        if n == 0 || after.len() < n {
            break;
        }
        out.push_str(&after[..n]);
        s = after[n..].trim_start_matches("\r\n");
    }
    out
}

// ---------- prompt ----------

pub const SYSTEM: &str = r#"You read one short sentence and fill in a to-do card. Answer with JSON only.
Use the given today date to work out dates. Dates are YYYY-MM-DD, times are 24-hour HH:MM.
title: the task in a few words, without the date or time words. date/time: null when the sentence gives none. all_day: true when there is a date but no time.
priority: "high" for urgent or important, "low" for someday, else "medium".
reminder_min: minutes before the due time, or null. repeat: "every day", "every weekday", "every week on mon and wed", "every month on the 1st", or null.
labels: only from the list you are given. duration_min: minutes, or null.
Examples (today is Tuesday 2026-11-24):
Call mum tomorrow at 5pm -> {"title":"Call mum","date":"2026-11-25","time":"17:00","all_day":false,"priority":"medium","reminder_min":null,"repeat":null,"labels":["People"],"duration_min":null}
Pay rent every month on the 1st, high priority -> {"title":"Pay rent","date":"2026-12-01","time":null,"all_day":true,"priority":"high","reminder_min":null,"repeat":"every month on the 1st","labels":["Money","Home"],"duration_min":null}
Gym every weekday at 7am -> {"title":"Gym","date":"2026-11-25","time":"07:00","all_day":false,"priority":"medium","reminder_min":null,"repeat":"every weekday","labels":["Health"],"duration_min":60}
Submit report by Friday urgent -> {"title":"Submit report","date":"2026-11-27","time":null,"all_day":true,"priority":"high","reminder_min":null,"repeat":null,"labels":["Work"],"duration_min":null}
Buy milk -> {"title":"Buy milk","date":null,"time":null,"all_day":false,"priority":"medium","reminder_min":null,"repeat":null,"labels":["Home"],"duration_min":null}"#;

pub fn schema(label_names: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "date": { "type": ["string", "null"] },
            "time": { "type": ["string", "null"] },
            "all_day": { "type": "boolean" },
            "priority": { "type": "string", "enum": ["low", "medium", "high"] },
            "reminder_min": { "type": ["integer", "null"] },
            "repeat": { "type": ["string", "null"] },
            "labels": { "type": "array", "items": { "type": "string", "enum": label_names } },
            "duration_min": { "type": ["integer", "null"] }
        },
        "required": ["title", "date", "time", "all_day", "priority", "reminder_min", "repeat", "labels", "duration_min"]
    })
}

pub fn user_message(text: &str, now: NaiveDateTime, label_names: &[&str]) -> String {
    format!(
        "Today is {} {}. Labels: {}.\nSentence: {}",
        now.format("%A"),
        now.format("%Y-%m-%d"),
        label_names.join(", "),
        text
    )
}

// ---------- checking the model's fields ----------

/// The result of reading one sentence.
#[derive(Clone, Debug)]
pub struct Reading {
    pub parsed: Parsed,
    /// Colour keys (see `store::LABELS`), from the model only.
    pub labels: Vec<String>,
    /// "ai" when the model answered with a usable object, else "rules".
    pub source: &'static str,
    /// Which fields came from the model (the others came from the rules).
    pub ai_fields: Vec<&'static str>,
    /// Why the rules answered alone (empty when the model answered).
    pub reason: String,
    pub ms: u128,
    /// The model's own text (cut to 600 letters), for checking what it said.
    pub raw: String,
}

impl Reading {
    pub fn to_json(&self) -> Value {
        let mut v = self.parsed.to_json();
        v["labels"] = json!(self.labels);
        v["source"] = json!(self.source);
        v["ai_fields"] = json!(self.ai_fields);
        v["reason"] = json!(self.reason);
        v["ms"] = json!(self.ms as u64);
        v["raw"] = json!(self.raw);
        v
    }
}

fn rules_only(p: Parsed, why: &str, ms: u128) -> Reading {
    Reading { parsed: p, labels: vec![], source: "rules", ai_fields: vec![], reason: why.into(), ms, raw: String::new() }
}

fn clean_title(v: &Value) -> Option<String> {
    let t: String = v.as_str()?.chars().filter(|c| !c.is_control()).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty() && t.chars().count() <= MAX_TITLE_CHARS).then_some(t)
}

fn has_any(text: &str, words: &[&str]) -> bool {
    let low = text.to_lowercase();
    low.split(|c: char| !c.is_alphanumeric()).any(|w| words.contains(&w))
}
fn has_date_cue(text: &str) -> bool {
    text.split_whitespace().any(|w| w.chars().any(|c| c.is_ascii_digit()) && w.contains(|c| c == '/' || c == '-' || c == '.'))
        || has_any(text, &["today", "tonight", "tomorrow", "week", "weekend", "month", "year", "next", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "mon", "tue", "wed", "thu", "fri", "sat", "sun", "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec", "january", "february", "march", "april", "june", "july", "august", "september", "october", "november", "december", "daily", "weekday", "weekdays", "every", "later"])
}
fn has_time_cue(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit()) || has_any(text, &["noon", "midnight", "morning", "afternoon", "evening", "night", "tonight", "lunch", "dinner", "breakfast"])
}

/// Null, empty text or the word "none" mean "the model found nothing". Returns None for those.
fn present(v: &Value) -> Option<&str> {
    let s = v.as_str()?.trim();
    (!s.is_empty() && !s.eq_ignore_ascii_case("null") && !s.eq_ignore_ascii_case("none")).then_some(s)
}

/// Check every field of the model's answer and merge it over the rules' reading.
/// Any field that is missing, empty or invalid keeps the rules' value.
pub fn merge(rules: Parsed, answer: &Value, text: &str, now: NaiveDateTime, ms: u128) -> Reading {
    if !answer.is_object() {
        return rules_only(rules, ReadErr::Junk.why(), ms);
    }
    let mut p = rules.clone();
    let mut used: Vec<&'static str> = Vec::new();
    let today = now.date();

    // Measured on this 0.8B model: the rules read dates, times, repeats and reminders better, so the model
    // only fills those when the rules found nothing. Its title counts only when it is not longer than the rules' title.
    if let Some(t) = clean_title(&answer["title"]).filter(|t| t.chars().count() <= rules.title.chars().count()) {
        if t != rules.title {
            used.push("title");
        }
        p.title = t;
    }
    // date: valid, not before today, within 10 years
    let mut ai_date = None;
    if let Some(d) = present(&answer["date"]).and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()) {
        // A small model often answers "today" when the sentence names no day: ask for a cue in the sentence.
        if d >= today && d <= today + Duration::days(3650) && (d != today || has_any(text, &["today", "tonight", "now"])) && has_date_cue(text) && rules.date.is_none() && parser::impossible_date(text).is_none() {
            ai_date = Some(d);
        }
    }
    if let Some(d) = ai_date {
        p.date = Some(d);
        used.push("date");
    }
    let all_day = answer["all_day"].as_bool() == Some(true);
    if let Some(t) = present(&answer["time"]).and_then(|s| NaiveTime::parse_from_str(s, "%H:%M").ok()) {
        // a time belongs to a date; the model's date wins, otherwise the rules' date
        // refuse the clock's own time (a small model copies it) and any time when the sentence has no time cue
        if !all_day && p.date.is_some() && has_time_cue(text) && rules.time.is_none() && Some(t) != NaiveTime::from_hms_opt(now.hour(), now.minute(), 0) {
            p.time = Some(t);
            used.push("time");
        }
    } else if all_day && ai_date.is_some() {
        p.time = None;
        used.push("time");
    }
    let wants_priority = has_any(text, &["urgent", "important", "asap", "critical", "high", "low", "medium", "someday", "priority", "whenever"]);
    match answer["priority"].as_str().map(str::to_lowercase).as_deref().filter(|_| wants_priority) {
        Some("low") => { p.priority = 0; used.push("priority"); }
        Some("medium") => { p.priority = 1; used.push("priority"); }
        Some("high") => { p.priority = 2; used.push("priority"); }
        _ => {}
    }
    // a small model invents reminders and repeats: ask for a word in the sentence that says so
    let wants_reminder = has_any(text, &["before", "remind", "reminder", "alert", "notify", "ahead"]);
    let wants_repeat = has_any(text, &["every", "daily", "weekly", "monthly", "yearly", "weekday", "weekdays", "each", "annually"]);
    if let Some(n) = answer["reminder_min"].as_i64().filter(|n| wants_reminder && rules.reminder_min.is_none() && (-1..=10_080).contains(n)) {
        p.reminder_min = Some(n);
        used.push("reminder");
    }
    if let Some(r) = present(&answer["repeat"]).filter(|_| wants_repeat && rules.repeat.is_none()).and_then(|s| parser::parse_repeat_phrase(s, now)) {
        p.repeat = Some(r);
        used.push("repeat");
    }
    if let Some(n) = answer["duration_min"].as_i64().filter(|n| (5..=1440).contains(n) && !rules.dur_set) {
        p.dur_min = n;
        used.push("duration");
    }
    // a repeat needs a start date
    if p.repeat.is_some() && p.date.is_none() {
        p.date = p.repeat.as_ref().map(|r| repeat::first_on_or_after(r, today));
    }
    if p.date.is_none() {
        p.time = None;
        p.repeat = None;
    }
    // labels: only the known names, once each, at most 3
    let mut labels: Vec<String> = Vec::new();
    for n in answer["labels"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if let Some((key, _)) = LABEL_TITLES.iter().find(|(_, t)| t.eq_ignore_ascii_case(n.trim())) {
            if !labels.iter().any(|l| l == key) && labels.len() < 3 {
                labels.push((*key).to_string());
            }
        }
    }
    // more than 3 labels at once is the model guessing: use none
    if answer["labels"].as_array().map(|a| a.len()).unwrap_or(0) > 3 {
        labels.clear();
    }
    if !labels.is_empty() {
        used.push("labels");
    }
    // the AI chip means the model's answer was used for at least one field
    let source = if used.is_empty() { "rules" } else { "ai" };
    let reason = if used.is_empty() { "the model's answer had nothing usable".to_string() } else { String::new() };
    Reading { parsed: p, labels, source, ai_fields: used, reason, ms, raw: String::new() }
}

/// Read one sentence. The rules always run first and give the fallback values.
pub fn read(reader: Option<&dyn Reader>, text: &str, now: NaiveDateTime) -> Reading {
    let rules = parser::parse(text, now);
    let Some(reader) = reader else { return rules_only(rules, "the AI reader is off", 0) };
    let names: Vec<&str> = LABEL_TITLES.iter().map(|(_, t)| *t).collect();
    let started = Instant::now();
    let out = reader.ask(SYSTEM, &user_message(text, now, &names), &schema(&names));
    let ms = started.elapsed().as_millis();
    match out {
        Err(e) => rules_only(rules, e.why(), ms),
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v) => Reading { raw: s.chars().take(600).collect(), ..merge(rules, &v, text, now, ms) },
            Err(_) => Reading { raw: s.chars().take(600).collect(), ..rules_only(rules, ReadErr::Junk.why(), ms) },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 24).unwrap().and_hms_opt(16, 36, 0).unwrap()
    }

    struct Fake(Result<String, ReadErr>);
    impl Reader for Fake {
        fn ask(&self, _: &str, _: &str, _: &Value) -> Result<String, ReadErr> {
            match &self.0 {
                Ok(s) => Ok(s.clone()),
                Err(ReadErr::Down) => Err(ReadErr::Down),
                Err(ReadErr::Timeout) => Err(ReadErr::Timeout),
                Err(ReadErr::Junk) => Err(ReadErr::Junk),
                Err(ReadErr::Model(m)) => Err(ReadErr::Model(m.clone())),
            }
        }
    }
    fn run(ans: &str, text: &str) -> Reading {
        let text = match text { "Call John" => "Call John tomorrow at 5pm, urgent, remind me 30 min before", "Gym" => "Gym each morning", t => t };
        read(Some(&Fake(Ok(ans.into()))), text, now())
    }
    const GOOD: &str = r#"{"title":"Call John","date":"2026-11-25","time":"17:00","all_day":false,"priority":"high","reminder_min":30,"repeat":null,"labels":["People"],"duration_min":45}"#;

    #[test]
    fn valid_answer_is_used() {
        let r = run(GOOD, "Call John");
        assert_eq!(r.source, "ai");
        assert_eq!(r.parsed.title, "Call John");
        assert_eq!(r.parsed.date, NaiveDate::from_ymd_opt(2026, 11, 25));
        assert_eq!(r.parsed.time, NaiveTime::from_hms_opt(17, 0, 0));
        assert_eq!(r.parsed.priority, 2);
        assert_eq!(r.parsed.reminder_min, Some(30));
        assert_eq!(r.parsed.dur_min, 45);
        assert_eq!(r.labels, vec!["blue"]);
    }

    #[test]
    fn bad_date_falls_back_to_rules_for_that_field_only() {
        let a = GOOD.replace("2026-11-25", "2026-13-45");
        let r = run(&a, "Call John tomorrow urgent");
        assert_eq!(r.parsed.date, NaiveDate::from_ymd_opt(2026, 11, 25)); // the rules read "tomorrow"
        assert!(!r.ai_fields.contains(&"date"));
        assert_eq!(r.parsed.priority, 2); // other fields still come from the model
        assert!(r.ai_fields.contains(&"priority"));
        // a date in the past is also refused
        let r = run(&GOOD.replace("2026-11-25", "2020-01-01"), "Call John tomorrow");
        assert_eq!(r.parsed.date, NaiveDate::from_ymd_opt(2026, 11, 25));
    }

    #[test]
    fn unknown_labels_are_dropped() {
        let a = GOOD.replace(r#"["People"]"#, r#"["Hacking","money","Money"]"#);
        let r = run(&a, "Call John");
        assert_eq!(r.labels, vec!["green"]);
    }

    #[test]
    fn junk_answers_use_rules() {
        for junk in ["not json at all", "[1,2]", "42", "", "{\"x\":1"] {
            let r = run(junk, "Call John tomorrow at 5 PM");
            assert_eq!(r.source, "rules", "{junk}");
            assert_eq!(r.parsed, parser::parse("Call John tomorrow at 5 PM", now()));
        }
        let r = run("{}", "Buy milk");
        assert_eq!(r.source, "rules");
    }

    #[test]
    fn errors_and_timeout_use_rules_with_a_reason() {
        for e in [ReadErr::Timeout, ReadErr::Down, ReadErr::Model("model not found".into())] {
            let want = e.why();
            let r = read(Some(&Fake(Err(e))), "Call John tomorrow at 5 PM", now());
            assert_eq!(r.source, "rules");
            assert_eq!(r.reason, want);
            assert_eq!(r.parsed.time, NaiveTime::from_hms_opt(17, 0, 0));
        }
        let r = read(None, "Buy milk", now());
        assert_eq!((r.source, r.reason.as_str()), ("rules", "the AI reader is off"));
    }

    #[test]
    fn field_by_field_checks() {
        // empty title -> rules' title; priority out of range -> rules'; reminder out of range -> rules'
        let a = r#"{"title":"   ","date":null,"time":"25:99","all_day":false,"priority":"urgent!!","reminder_min":-50,"repeat":"whenever","labels":"Money","duration_min":99999}"#;
        let r = run(a, "Pay rent tomorrow");
        assert_eq!(r.parsed.title, parser::parse("Pay rent tomorrow", now()).title);
        assert_eq!(r.parsed.priority, 1);
        assert_eq!(r.parsed.reminder_min, None);
        assert_eq!(r.parsed.repeat, None);
        assert_eq!(r.parsed.dur_min, parser::parse("Pay rent tomorrow", now()).dur_min);
        assert!(r.labels.is_empty());
        assert_eq!(r.source, "rules");
        // a 300-letter title is refused
        let long = format!(r#"{{"title":"{}","date":null,"time":null,"all_day":false,"priority":"low","reminder_min":null,"repeat":null,"labels":[],"duration_min":null}}"#, "a".repeat(300));
        let r = run(&long, "Buy milk, low priority");
        assert_eq!(r.parsed.title, parser::parse("Buy milk, low priority", now()).title);
        assert_eq!(r.parsed.priority, 0);
    }

    #[test]
    fn repeat_is_checked_by_the_repeat_parser() {
        let a = GOOD.replace(r#""repeat":null"#, r#""repeat":"every weekday""#);
        let r = run(&a, "Gym");
        assert_eq!(r.parsed.repeat, Some(repeat::Repeat::Weekdays));
        let a = GOOD.replace(r#""repeat":null"#, r#""repeat":"every blue moon""#);
        assert_eq!(run(&a, "Gym every blue moon").parsed.repeat, None);
        // a repeat with no date gets a start date; a time with no date is dropped
        let a = r#"{"title":"Gym","date":null,"time":null,"all_day":false,"priority":"medium","reminder_min":null,"repeat":"every day","labels":[],"duration_min":null}"#;
        assert!(run(a, "Gym").parsed.date.is_some());
        let a = r#"{"title":"Gym","date":null,"time":"07:00","all_day":false,"priority":"medium","reminder_min":null,"repeat":null,"labels":[],"duration_min":null}"#;
        assert_eq!(run(a, "Gym").parsed.time, None);
    }

    #[test]
    fn title_text_is_kept_as_plain_text() {
        let a = GOOD.replace("Call John", "<img src=x onerror=alert(1)>");
        let r = run(&a, "<img src=x onerror=alert(1)>");
        // stays text here; the page escapes it when drawing (view::esc, textContent in app.js)
        assert_eq!(r.parsed.title, "<img src=x onerror=alert(1)>");
    }

    #[test]
    fn only_this_computer_is_allowed() {
        assert_eq!(local_host_port("http://127.0.0.1:11434").as_deref(), Some("127.0.0.1:11434"));
        assert_eq!(local_host_port("http://localhost:11434/").as_deref(), Some("localhost:11434"));
        for bad in ["http://example.com:11434", "https://127.0.0.1:11434", "http://127.0.0.1", "http://10.0.0.5:11434", "http://127.0.0.1.evil.com:80", "127.0.0.1:11434"] {
            assert!(local_host_port(bad).is_none(), "{bad}");
        }
        assert!(valid_model("qwen3.5:0.8b") && !valid_model("a b") && !valid_model(""));
    }

    fn serve(reply: &'static [u8], hang: bool) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b);
                if hang {
                    std::thread::sleep(StdDuration::from_millis(1500));
                } else {
                    let _ = s.write_all(reply);
                }
            }
        });
        addr
    }

    #[test]
    fn real_client_reads_plain_and_chunked_answers_and_times_out() {
        let body = r#"{"message":{"content":"{\"title\":\"Hi\"}"}}"#;
        let plain: &'static [u8] = Box::leak(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes().into_boxed_slice());
        let o = Ollama { model: "m".into(), host_port: serve(plain, false), secs: 2 };
        assert_eq!(o.ask("s", "u", &json!({})).unwrap(), r#"{"title":"Hi"}"#);
        let chunked: &'static [u8] = Box::leak(format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len()).into_bytes().into_boxed_slice());
        let o = Ollama { model: "m".into(), host_port: serve(chunked, false), secs: 2 };
        assert_eq!(o.ask("s", "u", &json!({})).unwrap(), r#"{"title":"Hi"}"#);
        let o = Ollama { model: "m".into(), host_port: serve(b"", true), secs: 1 };
        let t = Instant::now();
        assert_eq!(o.ask("s", "u", &json!({})), Err(ReadErr::Timeout));
        assert!(t.elapsed() < StdDuration::from_millis(1400));
        // nothing listening
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        let o = Ollama { model: "m".into(), host_port: free, secs: 1 };
        assert_eq!(o.ask("s", "u", &json!({})), Err(ReadErr::Down));
    }

    #[test]
    fn ai_cannot_invent_a_date_or_change_a_range() {
        let a = r#"{"title":"Submit tax form","date":"2026-11-25","time":"18:00","all_day":false,"priority":"medium","reminder_min":null,"repeat":null,"labels":[],"duration_min":60}"#;
        let r = run(a, "Submit tax form on 30 February");
        assert_eq!(r.parsed.date, None);
        assert!(!r.ai_fields.contains(&"date") && !r.ai_fields.contains(&"time"));
        assert!(r.parsed.notes.iter().any(|n| n.contains("could not read the date")));
        // the rules' time range wins over the model's time and length
        let a = r#"{"title":"Meeting with Sara","date":"2026-11-30","time":"11:30","all_day":false,"priority":"medium","reminder_min":null,"repeat":null,"labels":[],"duration_min":60}"#;
        let r = run(a, "Meeting with Sara on Monday from 10 to 11:30");
        assert_eq!(r.parsed.time, NaiveTime::from_hms_opt(10, 0, 0));
        assert_eq!(r.parsed.dur_min, 90);
        // no date cue at all: a model date is refused
        let a = r#"{"title":"Buy 5 apples","date":"2026-11-25","time":null,"all_day":false,"priority":"medium","reminder_min":null,"repeat":null,"labels":[],"duration_min":null}"#;
        assert_eq!(run(a, "Buy 5 apples").parsed.date, None);
    }
}
