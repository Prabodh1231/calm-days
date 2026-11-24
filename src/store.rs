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
    /// The list (column) this card sits in.
    #[serde(default)]
    pub list_id: i64,
    /// Order inside the list, smallest first.
    #[serde(default)]
    pub pos: i64,
    #[serde(default)]
    pub notes: String,
    /// Colour label keys, comma separated (see `LABELS`).
    #[serde(default)]
    pub labels: String,
    #[serde(default)]
    pub checklist: Vec<Check>,
    #[serde(default)]
    pub archived: bool,
    /// Weekly cycle: the Monday (YYYY-MM-DD) of the week the card belongs to, or "".
    #[serde(default)]
    pub cycle: String,
}

/// The colour labels a card can carry.
pub const LABELS: [&str; 6] = ["green", "yellow", "orange", "red", "purple", "blue"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub t: String,
    pub d: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListRow {
    pub id: i64,
    pub name: String,
    pub pos: i64,
    pub archived: bool,
    /// Cards moved here count as finished.
    pub done_list: bool,
}

impl Item {
    pub fn label_keys(&self) -> Vec<&str> {
        self.labels.split(',').filter(|l| LABELS.contains(l)).collect()
    }
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
    #[serde(default)]
    pub restore_lists: Vec<ListRow>,
    #[serde(default)]
    pub remove_lists: Vec<i64>,
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

const COLS: &str = "id,title,kind,at,has_time,dur_min,deadline,priority,reminder_min,remind_at,fired,acked,repeat,until,done,sample,list_id,pos,notes,labels,checklist,archived,cycle";

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
        list_id: r.get(16)?,
        pos: r.get(17)?,
        notes: r.get(18)?,
        labels: r.get(19)?,
        checklist: r.get::<_, String>(20).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
        archived: r.get::<_, i64>(21)? != 0,
        cycle: r.get(22)?,
    })
}

impl Db {
    pub fn open(path: &str) -> rusqlite::Result<Db> {
        let db = Db::init(Connection::open(path)?)?;
        db.ensure_inbox()?;
        Ok(db)
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
               done INTEGER NOT NULL DEFAULT 0, sample INTEGER NOT NULL DEFAULT 0,
               list_id INTEGER NOT NULL DEFAULT 0, pos INTEGER NOT NULL DEFAULT 0, notes TEXT NOT NULL DEFAULT '',
               labels TEXT NOT NULL DEFAULT '', checklist TEXT NOT NULL DEFAULT '[]', archived INTEGER NOT NULL DEFAULT 0,
               cycle TEXT NOT NULL DEFAULT '');
             CREATE TABLE IF NOT EXISTS activity(
               id INTEGER PRIMARY KEY AUTOINCREMENT, item_id INTEGER NOT NULL, text TEXT NOT NULL, at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS lists(
               id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, pos INTEGER NOT NULL DEFAULT 0,
               archived INTEGER NOT NULL DEFAULT 0, done_list INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS completions(
               id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, priority INTEGER NOT NULL, done_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS proposals(
               id INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', created INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS undo_log(
               id INTEGER PRIMARY KEY AUTOINCREMENT, descr TEXT NOT NULL, payload TEXT NOT NULL, created INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0);",
        )?;
        // Old Calm Days databases have no board columns: add them, keep every item.
        let have: Vec<String> = conn.prepare("PRAGMA table_info(items)")?.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<_, _>>()?;
        let migrating = !have.iter().any(|h| h == "list_id");
        for (col, ddl) in [
            ("list_id", "INTEGER NOT NULL DEFAULT 0"),
            ("pos", "INTEGER NOT NULL DEFAULT 0"),
            ("notes", "TEXT NOT NULL DEFAULT ''"),
            ("labels", "TEXT NOT NULL DEFAULT ''"),
            ("checklist", "TEXT NOT NULL DEFAULT '[]'"),
            ("archived", "INTEGER NOT NULL DEFAULT 0"),
            ("cycle", "TEXT NOT NULL DEFAULT ''"),
        ] {
            if !have.iter().any(|h| h == col) {
                conn.execute(&format!("ALTER TABLE items ADD COLUMN {col} {ddl}"), [])?;
            }
        }
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM lists", [], |r| r.get(0))?;
        if n == 0 {
            for (i, (name, done)) in [("To do", 0), ("Doing", 0), ("Done", 1)].iter().enumerate() {
                conn.execute("INSERT INTO lists(name,pos,done_list) VALUES(?,?,?)", params![name, i as i64, done])?;
            }
        }
        // Cards without a list: finished ones go to the done list, the rest to the first list.
        conn.execute(
            "UPDATE items SET list_id=(SELECT id FROM lists WHERE done_list=1 ORDER BY pos LIMIT 1) WHERE list_id=0 AND done=1",
            [],
        )?;
        conn.execute("UPDATE items SET list_id=(SELECT id FROM lists WHERE done_list=0 ORDER BY pos LIMIT 1) WHERE list_id=0", [])?;
        if migrating {
            conn.execute("UPDATE items SET pos=id", [])?;
        }
        Ok(Db { conn })
    }

    /// Insert a new card (id is ignored) and return its id.
    pub fn insert(&self, it: &Item) -> rusqlite::Result<i64> {
        let mut c = it.clone();
        c.id = 0;
        self.put(&c)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert or overwrite by id (used by save and undo). id 0 means a new row.
    pub fn put(&self, it: &Item) -> rusqlite::Result<()> {
        let id: Option<i64> = if it.id == 0 { None } else { Some(it.id) };
        self.conn.execute(
            "INSERT OR REPLACE INTO items(id,title,kind,at,has_time,dur_min,deadline,priority,reminder_min,remind_at,fired,acked,repeat,until,done,sample,list_id,pos,notes,labels,checklist,archived,cycle)
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                id, it.title, it.kind, it.at, it.has_time, it.dur_min, it.deadline, it.priority, it.reminder_min, it.remind_at,
                it.fired, it.acked, it.repeat.as_ref().and_then(|r| serde_json::to_string(r).ok()),
                it.until.map(|d| d.format("%Y-%m-%d").to_string()), it.done, it.sample,
                it.list_id, it.pos, it.notes, it.labels, serde_json::to_string(&it.checklist).unwrap_or_else(|_| "[]".into()), it.archived, it.cycle
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
            "SELECT {COLS} FROM items WHERE archived=0 AND list_id NOT IN (SELECT id FROM lists WHERE archived=1) {} ORDER BY at IS NULL, at, id",
            if include_done { "" } else { "AND done=0" }
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

    // ---- activity log ----
    pub fn log_activity(&self, item_id: i64, text: &str, at: i64) -> rusqlite::Result<()> {
        self.conn.execute("INSERT INTO activity(item_id,text,at) VALUES(?,?,?)", params![item_id, text, at])?;
        Ok(())
    }
    /// Newest first: (text, UTC seconds).
    pub fn activity_for(&self, item_id: i64, limit: i64) -> rusqlite::Result<Vec<(String, i64)>> {
        let mut st = self.conn.prepare("SELECT text,at FROM activity WHERE item_id=? ORDER BY id DESC LIMIT ?")?;
        let rows = st.query_map(params![item_id, limit], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- inbox ----
    /// Make sure there is an "Inbox" list at the left. Runs once per database (an old database is
    /// migrated in place; archiving the Inbox later is respected).
    pub fn ensure_inbox(&self) -> rusqlite::Result<()> {
        if self.meta("inbox_id").is_some() {
            return Ok(());
        }
        let first = self.lists(true)?.iter().map(|l| l.pos).min().unwrap_or(0);
        let id = self.put_list(&ListRow { id: 0, name: "Inbox".into(), pos: first - 1, archived: false, done_list: false })?;
        self.set_meta("inbox_id", &id.to_string())
    }
    /// The Inbox list, when it exists and is not archived.
    pub fn inbox_id(&self) -> Option<i64> {
        let id: i64 = self.meta("inbox_id")?.parse().ok()?;
        self.get_list(id).ok().flatten().filter(|l| !l.archived).map(|l| l.id)
    }

    // ---- lists and card order ----
    pub fn lists(&self, with_archived: bool) -> rusqlite::Result<Vec<ListRow>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT id,name,pos,archived,done_list FROM lists {} ORDER BY pos,id",
            if with_archived { "" } else { "WHERE archived=0" }
        ))?;
        let rows = st
            .query_map([], |r| {
                Ok(ListRow { id: r.get(0)?, name: r.get(1)?, pos: r.get(2)?, archived: r.get::<_, i64>(3)? != 0, done_list: r.get::<_, i64>(4)? != 0 })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn get_list(&self, id: i64) -> rusqlite::Result<Option<ListRow>> {
        Ok(self.lists(true)?.into_iter().find(|l| l.id == id))
    }
    /// Insert or overwrite a list. id 0 means a new list.
    pub fn put_list(&self, l: &ListRow) -> rusqlite::Result<i64> {
        let id: Option<i64> = if l.id == 0 { None } else { Some(l.id) };
        self.conn.execute(
            "INSERT OR REPLACE INTO lists(id,name,pos,archived,done_list) VALUES(?,?,?,?,?)",
            params![id, l.name, l.pos, l.archived, l.done_list],
        )?;
        Ok(if l.id == 0 { self.conn.last_insert_rowid() } else { l.id })
    }
    pub fn delete_list(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM lists WHERE id=?", [id])?;
        Ok(())
    }
    /// Cards of one list in board order (not archived).
    pub fn cards_in(&self, list_id: i64) -> rusqlite::Result<Vec<Item>> {
        let mut st = self.conn.prepare(&format!("SELECT {COLS} FROM items WHERE list_id=? AND archived=0 ORDER BY pos,id"))?;
        let rows = st.query_map([list_id], row_item)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    /// Write `pos` 0..n for the cards in the given order.
    pub fn renumber(&self, ids: &[i64], list_id: i64) -> rusqlite::Result<()> {
        for (i, id) in ids.iter().enumerate() {
            self.conn.execute("UPDATE items SET pos=?, list_id=? WHERE id=?", params![i as i64, list_id, id])?;
        }
        Ok(())
    }
    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0)).optional().ok().flatten()
    }
    pub fn set_meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)", params![key, value])?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_database_is_migrated_and_keeps_its_items() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE items(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'task',
               at INTEGER, has_time INTEGER NOT NULL DEFAULT 0, dur_min INTEGER NOT NULL DEFAULT 30, deadline INTEGER,
               priority INTEGER NOT NULL DEFAULT 1, reminder_min INTEGER, remind_at INTEGER, fired INTEGER NOT NULL DEFAULT 0,
               acked INTEGER NOT NULL DEFAULT 0, repeat TEXT, until TEXT, done INTEGER NOT NULL DEFAULT 0, sample INTEGER NOT NULL DEFAULT 0);
             INSERT INTO items(title,done) VALUES('Open one',0);
             INSERT INTO items(title,done) VALUES('Finished one',1);",
        )
        .unwrap();
        let db = Db::init(conn).unwrap();
        let lists = db.lists(false).unwrap();
        assert_eq!(lists.len(), 3);
        let todo = lists.iter().find(|l| !l.done_list).unwrap().id;
        let done = lists.iter().find(|l| l.done_list).unwrap().id;
        let all = db.list(true).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().find(|i| i.title == "Open one").unwrap().list_id, todo);
        assert_eq!(all.iter().find(|i| i.title == "Finished one").unwrap().list_id, done);
    }
}
