use rusqlite::Connection;
use tauri::Runtime;

// Demo data lives here (not in a migration) so it can change freely without
// tripping sqlx's "migration was modified" checksum check. Same pattern as
// TyA_Tauri's seeder. Not called on startup yet — see lib.rs setup.
const SEED_SQL: &str = include_str!("../seed/defaults.sql");

#[allow(dead_code)]
pub fn seed_defaults<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(db_path) = crate::db::get_db_path(app) else {
        eprintln!("[seeder] no db path");
        return;
    };
    let conn = match Connection::open(db_path) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("[seeder] open failed: {}", err);
            return;
        }
    };
    match conn.execute_batch(SEED_SQL) {
        Ok(()) => println!("[seeder] defaults seeded"),
        Err(err) => eprintln!("[seeder] failed: {}", err),
    }
}
