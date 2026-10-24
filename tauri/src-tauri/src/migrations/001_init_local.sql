-- Local-only tables on the user's Mac. The server schema lives in
-- backend/schema.sql; raw activity never leaves this file, only a derived
-- topic is sent to the backend.

-- Single row: who is signed in on this device. NULL token = signed out.
CREATE TABLE IF NOT EXISTS curr_user_profile (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    server_user_id  INTEGER,                    -- users.id on the backend
    token           TEXT,                       -- secret sent in "hello"
    name            TEXT,
    school          TEXT,
    bio             TEXT,
    sharing_enabled INTEGER NOT NULL DEFAULT 0 CHECK (sharing_enabled IN (0, 1))
);

INSERT OR IGNORE INTO curr_user_profile (id) VALUES (1);

-- What the user was doing: one row per stretch of time in an app or a
-- calendar event. ended_at NULL = still going.
CREATE TABLE IF NOT EXISTS activity_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    source     TEXT    NOT NULL CHECK (source IN ('app', 'calendar')),
    app_name   TEXT,                            -- e.g. "Visual Studio Code"
    bundle_id  TEXT,                            -- e.g. "com.microsoft.VSCode"
    title      TEXT,                            -- calendar event title
    topic      TEXT,                            -- derived, e.g. "leetcode"
    started_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_activity_started ON activity_log (started_at);

-- Simple key/value app settings (pause matching, etc.).
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
