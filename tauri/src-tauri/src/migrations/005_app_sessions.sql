-- Which app (and window) was in front, and for how long. Written by
-- collecting_data/app_tracker.rs every 2 s while sharing is on; kept as a
-- local history. Never sent anywhere: only the 5-minute summary built from it
-- is pushed.
CREATE TABLE IF NOT EXISTS app_sessions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    app_name     TEXT    NOT NULL,          -- "Visual Studio Code"
    bundle_id    TEXT,                      -- "com.microsoft.VSCode"
    window_title TEXT,                      -- scrubbed; NULL without Accessibility or for skipped apps
    document     TEXT,                      -- "cs170-hw4/dijkstra.py" (parent folder + file only); NULL if none
    started_at   INTEGER NOT NULL,          -- unix ms
    ended_at     INTEGER NOT NULL           -- unix ms, last time seen in front (updated every 2 s)
);

CREATE INDEX IF NOT EXISTS idx_app_sessions_ended ON app_sessions (ended_at);
