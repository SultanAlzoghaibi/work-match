import { invoke } from "@tauri-apps/api/core";
import { getDb } from "./db.js";
import { getDb } from "./db.js";

// Same idea as TyA's getOs(): ask Rust for the target OS instead of sniffing
// the user agent. macOS gets native liquid glass, so the page stays clear;
// everything else paints a CSS semi-transparent background.
const os = await invoke("os_name").catch(() => "unknown");
document.documentElement.classList.add(os === "macos" ? "os-macos" : "os-other");

// Smoke test for the local SQLite DB: show how many activity rows exist.
try {
  const db = await getDb();
  const [{ count }] = await db.select("SELECT COUNT(*) AS count FROM activity_log");
  document.getElementById("db-status").textContent = `Local DB ready · ${count} activity rows`;
} catch (err) {
  document.getElementById("db-status").textContent = `Local DB error: ${err}`;
}
