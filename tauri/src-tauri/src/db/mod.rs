pub mod seeder;

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
