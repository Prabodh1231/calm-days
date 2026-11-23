//! SQLite storage (bundled). Times are stored as UTC seconds; the app reads them in the server's local zone.
use crate::approval::Change;
use crate::repeat::Repeat;
use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

pub fn now_local() -> NaiveDateTime {
    Local::now().naive_local()
}

/// Local wall-clock time -> UTC seconds. A time skipped by a clock change moves one hour later.
pub fn to_utc(n: NaiveDateTime) -> i64 {
    match Local.from_local_datetime(&n) {
        LocalResult::Single(t) => t.timestamp(),
        LocalResult::Ambiguous(a, _) => a.timestamp(),
        LocalResult::None => Local
            .from_local_datetime(&(n + Duration::hours(1)))
            .earliest()
            .map(|t| t.timestamp())
            .unwrap_or_else(|| n.and_utc().timestamp()),
    }
}

pub fn from_utc(s: i64) -> NaiveDateTime {
    DateTime::from_timestamp(s, 0).map(|d| d.with_timezone(&Local).naive_local()).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub title: String,
    /// "task" or "event" (a fixed event)
    pub kind: String,
    pub at: Option<i64>,
    /// false = only the date matters (all day)
    pub has_time: bool,
    pub dur_min: i64,
    pub deadline: Option<i64>,
    /// 0 low, 1 medium, 2 high
    pub priority: i64,
    pub reminder_min: Option<i64>,
    pub remind_at: Option<i64>,
    pub fired: bool,
    pub acked: bool,
    pub repeat: Option<Repeat>,
    pub until: Option<NaiveDate>,
    pub done: bool,
    pub sample: bool,
}

impl Item {
    pub fn at_local(&self) -> Option<NaiveDateTime> {
        self.at.map(from_utc)
    }
    pub fn deadline_local(&self) -> Option<NaiveDateTime> {
        self.deadline.map(from_utc)
    }
    pub fn is_event(&self) -> bool {
        self.kind == "event"
    }
    pub fn is_overdue(&self, now: NaiveDateTime) -> bool {
        if self.done || self.is_event() {
            return false;
        }
        match self.at_local() {
            Some(a) if self.has_time => a < now,
            Some(a) => a.date() < now.date(),
            None => false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Undo {
    pub restore: Vec<Item>,
    pub remove_items: Vec<i64>,
    pub remove_completions: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct Completion {
    pub title: String,
    pub priority: i64,
    pub done_at: i64,
}

#[derive(Clone, Debug)]
pub struct UndoRow {
    pub id: i64,
    pub desc: String,
    pub undone: bool,
}

pub struct Db {
    pub conn: Connection,
}

const COLS: &str = "id,title,kind,at,has_time,dur_min,deadline,priority,reminder_min,remind_at,fired,acked,repeat,until,done,sample";

fn row_item(r: &Row) -> rusqlite::Result<Item> {
    let repeat: Option<String> = r.get(12)?;
    let until: Option<String> = r.get(13)?;
    Ok(Item {
        id: r.get(0)?,
        title: r.get(1)?,
        kind: r.get(2)?,
        at: r.get(3)?,
        has_time: r.get::<_, i64>(4)? != 0,
        dur_min: r.get(5)?,
        deadline: r.get(6)?,
        priority: r.get(7)?,
        reminder_min: r.get(8)?,
        remind_at: r.get(9)?,
        fired: r.get::<_, i64>(10)? != 0,
        acked: r.get::<_, i64>(11)? != 0,
        repeat: repeat.and_then(|s| serde_json::from_str(&s).ok()),
        until: until.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        done: r.get::<_, i64>(14)? != 0,
        sample: r.get::<_, i64>(15)? != 0,
    })
}

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Db> {
        Db::init(Connection::open(path)?)
    }
    pub fn memory() -> rusqlite::Result<Db> {
        Db::init(Connection::open_in_memory()?)
    }
    fn init(conn: Connection) -> rusqlite::Result<Db> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS items(
               id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'task',
               at INTEGER, has_time INTEGER NOT NULL DEFAULT 0, dur_min INTEGER NOT NULL DEFAULT 30,
               deadline INTEGER, priority INTEGER NOT NULL DEFAULT 1, reminder_min INTEGER, remind_at INTEGER,
               fired INTEGER NOT NULL DEFAULT 0, acked INTEGER NOT NULL DEFAULT 0, repeat TEXT, until TEXT,
               done INTEGER NOT NULL DEFAULT 0, sample INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS completions(
               id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, priority INTEGER NOT NULL, done_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS proposals(
               id INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS undo_log(
               id INTEGER PRIMARY KEY AUTOINCREMENT, descr TEXT NOT NULL, payload TEXT NOT NULL, created INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(Db { conn })
    }

    pub fn insert(&self, it: &Item) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO items(title,kind,at,has_time,dur_min,deadline,priority,reminder_min,remind_at,fired,acked,repeat,until,done,sample)
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                it.title, it.kind, it.at, it.has_time, it.dur_min, it.deadline, it.priority, it.reminder_min, it.remind_at,
                it.fired, it.acked, it.repeat.as_ref().and_then(|r| serde_json::to_string(r).ok()),
                it.until.map(|d| d.format("%Y-%m-%d").to_string()), it.done, it.sample
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert or overwrite by id (used by save and undo).
    pub fn put(&self, it: &Item) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO items(id,title,kind,at,has_time,dur_min,deadline,priority,reminder_min,remind_at,fired,acked,repeat,until,done,sample)
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                it.id, it.title, it.kind, it.at, it.has_time, it.dur_min, it.deadline, it.priority, it.reminder_min, it.remind_at,
                it.fired, it.acked, it.repeat.as_ref().and_then(|r| serde_json::to_string(r).ok()),
                it.until.map(|d| d.format("%Y-%m-%d").to_string()), it.done, it.sample
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> rusqlite::Result<Option<Item>> {
        self.conn.query_row(&format!("SELECT {COLS} FROM items WHERE id=?"), [id], row_item).optional()
    }

    pub fn delete(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM items WHERE id=?", [id])?;
        Ok(())
    }

    /// Open items (and done ones when asked), ordered by time then id.
    pub fn list(&self, include_done: bool) -> rusqlite::Result<Vec<Item>> {
        let sql = format!(
            "SELECT {COLS} FROM items {} ORDER BY at IS NULL, at, id",
            if include_done { "" } else { "WHERE done=0" }
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], row_item)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn add_completion(&self, title: &str, priority: i64, done_at: i64) -> rusqlite::Result<i64> {
        self.conn.execute("INSERT INTO completions(title,priority,done_at) VALUES(?,?,?)", params![title, priority, done_at])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn remove_completion(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM completions WHERE id=?", [id])?;
        Ok(())
    }
    pub fn completions_between(&self, from: i64, to: i64) -> rusqlite::Result<Vec<Completion>> {
        let mut st = self.conn.prepare("SELECT title,priority,done_at FROM completions WHERE done_at>=? AND done_at<? ORDER BY done_at")?;
        let rows = st
            .query_map(params![from, to], |r| Ok(Completion { title: r.get(0)?, priority: r.get(1)?, done_at: r.get(2)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- proposals ----
    pub fn add_proposal(&self, c: &Change, now: i64) -> rusqlite::Result<i64> {
        let payload = serde_json::to_string(c).unwrap_or_default();
        self.conn.execute("INSERT INTO proposals(payload,created) VALUES(?,?)", params![payload, now])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn get_proposal(&self, id: i64) -> rusqlite::Result<Option<(Change, String)>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row("SELECT payload,status FROM proposals WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.and_then(|(p, s)| serde_json::from_str(&p).ok().map(|c| (c, s))))
    }
    pub fn set_proposal_status(&self, id: i64, status: &str) -> rusqlite::Result<()> {
        self.conn.execute("UPDATE proposals SET status=? WHERE id=?", params![status, id])?;
        Ok(())
    }
    pub fn pending_proposals(&self) -> rusqlite::Result<Vec<(i64, Change)>> {
        let mut st = self.conn.prepare("SELECT id,payload FROM proposals WHERE status='pending' ORDER BY id")?;
        let rows: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(rows.into_iter().filter_map(|(id, p)| serde_json::from_str(&p).ok().map(|c| (id, c))).collect())
    }

    // ---- undo log ----
    pub fn add_undo(&self, desc: &str, u: &Undo, now: i64) -> rusqlite::Result<i64> {
        let payload = serde_json::to_string(u).unwrap_or_default();
        self.conn.execute("INSERT INTO undo_log(descr,payload,created) VALUES(?,?,?)", params![desc, payload, now])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn get_undo(&self, id: i64) -> rusqlite::Result<Option<(Undo, bool)>> {
        let row: Option<(String, i64)> = self
            .conn
            .query_row("SELECT payload,undone FROM undo_log WHERE id=?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.and_then(|(p, u)| serde_json::from_str(&p).ok().map(|x| (x, u != 0))))
    }
    pub fn mark_undone(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("UPDATE undo_log SET undone=1 WHERE id=?", [id])?;
        Ok(())
    }
    pub fn recent_undo(&self, n: i64) -> rusqlite::Result<Vec<UndoRow>> {
        let mut st = self.conn.prepare("SELECT id,descr,undone FROM undo_log ORDER BY id DESC LIMIT ?")?;
        let rows = st
            .query_map([n], |r| Ok(UndoRow { id: r.get(0)?, desc: r.get(1)?, undone: r.get::<_, i64>(2)? != 0 }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- reminders ----
    /// Mark reminders whose time has come. Returns how many fired.
    pub fn check_due(&self, now_utc: i64) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE items SET fired=1 WHERE done=0 AND fired=0 AND remind_at IS NOT NULL AND remind_at<=?",
            [now_utc],
        )
    }
    pub fn due_list(&self) -> rusqlite::Result<Vec<Item>> {
        let mut st = self.conn.prepare(&format!("SELECT {COLS} FROM items WHERE done=0 AND fired=1 AND acked=0 ORDER BY remind_at, id"))?;
        let rows = st.query_map([], row_item)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn ack(&self, ids: &[i64]) -> rusqlite::Result<()> {
        for id in ids {
            self.conn.execute("UPDATE items SET acked=1 WHERE id=?", [id])?;
        }
        Ok(())
    }
}
