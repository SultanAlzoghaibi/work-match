pub mod seeder;

use rusqlite::Connection;
use tauri::{Manager, Runtime};

/// File name part of crate::DB_FILE ("sqlite:work_match.db").
pub const DB_FILENAME: &str = "work_match.db";

/// Where tauri-plugin-sql keeps the DB: the app config dir
/// (~/Library/Application Support/com.sultanalzoghaibi.workmatch on macOS).
pub fn get_db_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|config_dir| config_dir.join(DB_FILENAME))
}

/// curr_user_profile.sharing_enabled, or None if the DB can't be read. The sql
/// plugin preloads the DB and runs migration 001 before app setup, so the row exists.
pub fn load_sharing_enabled<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<bool> {
    let conn = Connection::open(get_db_path(app)?).ok()?;
    conn.query_row(
        "SELECT sharing_enabled FROM curr_user_profile WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .ok()
}

pub fn save_sharing_enabled<R: Runtime>(app: &tauri::AppHandle<R>, enabled: bool) {
    let Some(db_path) = get_db_path(app) else {
        eprintln!("[db] no db path");
        return;
    };
    let result = Connection::open(db_path).and_then(|conn| {
        conn.execute(
            "UPDATE curr_user_profile SET sharing_enabled = ?1 WHERE id = 1",
            [enabled],
        )
    });
    if let Err(err) = result {
        eprintln!("[db] saving sharing_enabled failed: {}", err);
    }
}
