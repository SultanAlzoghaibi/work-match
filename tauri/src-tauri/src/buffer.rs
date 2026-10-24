// Local activity buffer: a SQLite file holding at most the last few minutes of
// captured page text. Only the summarizer reads captures, and it deletes every
// row it read. Nothing here ever leaves the device.
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const FILE_NAME: &str = "activity_buffer.sqlite";
/// Rows older than this are swept even when no summary ran (model down, laptop asleep).
const RETENTION_MS: i64 = 6 * 60 * 1000;
const SWEEP_EVERY: Duration = Duration::from_secs(60);

const SCHEMA: &str = "
PRAGMA secure_delete = ON;   -- overwrite deleted content on disk
PRAGMA journal_mode = DELETE;

CREATE TABLE IF NOT EXISTS captures (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    captured_at INTEGER NOT NULL,          -- unix ms, from server receive time
    source      TEXT    NOT NULL,          -- 'chrome' (extension) or 'screen' (screen_capture.rs)
    host        TEXT    NOT NULL,
    path        TEXT    NOT NULL,
    title       TEXT    NOT NULL,
    text        TEXT    NOT NULL,
    text_hash   TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_captures_time ON captures (captured_at);

CREATE TABLE IF NOT EXISTS last_summary (   -- single row, for the sharing panel
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    created_at  INTEGER NOT NULL,
    json        TEXT    NOT NULL
);
";

pub struct NewCapture<'a> {
    pub captured_at: i64,
    pub source: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub text: &'a str,
    pub text_hash: &'a str,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub id: i64,
    pub captured_at: i64,
    /// 'chrome' or 'screen'. For 'screen', host is the app name and title the window title.
    pub source: String,
    pub host: String,
    pub path: String,
    pub title: String,
    pub text: String,
}

#[derive(Clone)]
pub struct Buffer {
    conn: Arc<Mutex<Connection>>,
}

impl Buffer {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Self {
        Self::init(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // A panic can't leave a statement half-applied, so a poisoned lock is still safe to use.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns false, storing nothing, when the newest row for the same page has the same text.
    pub fn insert(&self, capture: &NewCapture) -> rusqlite::Result<bool> {
        let conn = self.conn();
        let last_hash: Option<String> = conn
            .query_row(
                "SELECT text_hash FROM captures WHERE host = ?1 AND path = ?2 ORDER BY id DESC LIMIT 1",
                params![capture.host, capture.path],
                |row| row.get(0),
            )
            .optional()?;
        if last_hash.as_deref() == Some(capture.text_hash) {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO captures (captured_at, source, host, path, title, text, text_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                capture.captured_at,
                capture.source,
                capture.host,
                capture.path,
                capture.title,
                capture.text,
                capture.text_hash
            ],
        )?;
        Ok(true)
    }

    /// Whether `source` stored anything since `since_ms`. screen_capture.rs
    /// skips Chrome while the extension is sending pages.
    pub fn has_recent(&self, source: &str, since_ms: i64) -> bool {
        self.conn()
            .query_row(
                "SELECT 1 FROM captures WHERE source = ?1 AND captured_at >= ?2 LIMIT 1",
                params![source, since_ms],
                |_| Ok(()),
            )
            .optional()
            .is_ok_and(|row| row.is_some())
    }

    pub fn read_all(&self) -> rusqlite::Result<Vec<Capture>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, captured_at, source, host, path, title, text FROM captures ORDER BY captured_at, id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Capture {
                id: row.get(0)?,
                captured_at: row.get(1)?,
                source: row.get(2)?,
                host: row.get(3)?,
                path: row.get(4)?,
                title: row.get(5)?,
                text: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Deletes the rows a summary read; rows inserted since then have larger ids and stay.
    pub fn delete_through(&self, max_id: i64) -> rusqlite::Result<usize> {
        self.conn()
            .execute("DELETE FROM captures WHERE id <= ?1", [max_id])
    }

    pub fn sweep(&self, now_ms: i64) -> rusqlite::Result<usize> {
        self.conn().execute(
            "DELETE FROM captures WHERE captured_at < ?1",
            [now_ms - RETENTION_MS],
        )
    }

    pub fn clear_captures(&self) -> rusqlite::Result<()> {
        self.conn().execute_batch("DELETE FROM captures;")
    }

    pub fn clear_all(&self) -> rusqlite::Result<()> {
        self.conn()
            .execute_batch("DELETE FROM captures; DELETE FROM last_summary;")
    }

    pub fn save_last_summary(&self, created_at: i64, json: &str) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO last_summary (id, created_at, json) VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET created_at = excluded.created_at, json = excluded.json",
            params![created_at, json],
        )?;
        Ok(())
    }

    pub fn last_summary(&self) -> rusqlite::Result<Option<String>> {
        self.conn()
            .query_row("SELECT json FROM last_summary WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

pub fn start_sweeper(buffer: Buffer) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(SWEEP_EVERY);
        loop {
            ticker.tick().await;
            // A failed sweep is retried on the next tick.
            if let Err(err) = buffer.sweep(now_ms()) {
                eprintln!("[buffer] sweep failed: {err}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture<'a>(captured_at: i64, path: &'a str, text: &'a str, hash: &'a str) -> NewCapture<'a> {
        NewCapture {
            captured_at,
            source: "chrome",
            host: "leetcode.com",
            path,
            title: "Course Schedule - LeetCode",
            text,
            text_hash: hash,
        }
    }

    #[test]
    fn insert_and_read_in_time_order() {
        let buffer = Buffer::open_in_memory();
        assert!(buffer.insert(&capture(2_000, "/b", "second", "h2")).unwrap());
        assert!(buffer.insert(&capture(1_000, "/a", "first", "h1")).unwrap());
        let rows = buffer.read_all().unwrap();
        let texts: Vec<_> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(texts, ["first", "second"]);
    }

    #[test]
    fn skips_unchanged_text_for_the_same_page() {
        let buffer = Buffer::open_in_memory();
        assert!(buffer.insert(&capture(1_000, "/a", "same", "h1")).unwrap());
        assert!(!buffer.insert(&capture(2_000, "/a", "same", "h1")).unwrap());
        assert!(buffer.insert(&capture(3_000, "/b", "same", "h1")).unwrap());
        assert!(buffer.insert(&capture(4_000, "/a", "changed", "h2")).unwrap());
        assert_eq!(buffer.read_all().unwrap().len(), 3);
    }

    #[test]
    fn delete_through_keeps_newer_rows() {
        let buffer = Buffer::open_in_memory();
        buffer.insert(&capture(1_000, "/a", "a", "h1")).unwrap();
        buffer.insert(&capture(2_000, "/b", "b", "h2")).unwrap();
        let max_id = buffer.read_all().unwrap().last().unwrap().id;
        buffer.insert(&capture(3_000, "/c", "c", "h3")).unwrap();

        assert_eq!(buffer.delete_through(max_id).unwrap(), 2);
        let rows = buffer.read_all().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "/c");
    }

    #[test]
    fn sweep_deletes_rows_older_than_six_minutes() {
        let buffer = Buffer::open_in_memory();
        let now = 10 * 60 * 1000;
        buffer.insert(&capture(now - RETENTION_MS - 1, "/old", "old", "h1")).unwrap();
        buffer.insert(&capture(now - RETENTION_MS, "/edge", "edge", "h2")).unwrap();
        buffer.insert(&capture(now - 1_000, "/new", "new", "h3")).unwrap();

        assert_eq!(buffer.sweep(now).unwrap(), 1);
        let paths: Vec<_> = buffer.read_all().unwrap().into_iter().map(|row| row.path).collect();
        assert_eq!(paths, ["/edge", "/new"]);
    }

    #[test]
    fn clear_all_empties_both_tables() {
        let buffer = Buffer::open_in_memory();
        buffer.insert(&capture(1_000, "/a", "a", "h1")).unwrap();
        buffer.save_last_summary(1_000, "{}").unwrap();

        buffer.clear_captures().unwrap();
        assert!(buffer.read_all().unwrap().is_empty());
        assert!(buffer.last_summary().unwrap().is_some());

        buffer.clear_all().unwrap();
        assert!(buffer.last_summary().unwrap().is_none());
    }

    #[test]
    fn last_summary_keeps_one_row() {
        let buffer = Buffer::open_in_memory();
        buffer.save_last_summary(1_000, r#"{"n":1}"#).unwrap();
        buffer.save_last_summary(2_000, r#"{"n":2}"#).unwrap();
        assert_eq!(buffer.last_summary().unwrap().as_deref(), Some(r#"{"n":2}"#));
    }
}
