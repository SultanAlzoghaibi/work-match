import Database from "@tauri-apps/plugin-sql";

// Same file as DB_FILE in src-tauri/src/lib.rs. Rust runs the migrations;
// this just opens a connection for queries from the UI.
let dbPromise;
export function getDb() {
  dbPromise ??= Database.load("sqlite:work_match.db");
  return dbPromise;
}
