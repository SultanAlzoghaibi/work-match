// Read-only views for the app's Dev tab: is tracking actually running, what
// was captured, what the summarizer read and returned, and what was pushed.
// Nothing here is sent anywhere; it only reads local state.
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

/// Shared with the summarizer (last run, next tick) and api.rs (last push).
#[derive(Clone, Default)]
pub struct DevState(Arc<Inner>);

#[derive(Default)]
struct Inner {
    last_run: Mutex<Option<LastRun>>,
    last_push: Mutex<Option<serde_json::Value>>,
    /// Unix ms of the next scheduled summary (0 = not scheduled yet).
    next_summary_at: AtomicI64,
}

/// One summarizer run, as the Dev tab shows it.
#[derive(Clone, serde::Serialize)]
pub struct LastRun {
    pub at: i64,
    /// Exactly what the model read; None when there was nothing to summarize.
    pub prompt: Option<String>,
    /// Raw model replies or errors, one per attempt.
    pub replies: Vec<String>,
    pub summary: serde_json::Value,
}

impl DevState {
    fn forget_runs(&self) {
        *self.0.last_run.lock().unwrap() = None;
        *self.0.last_push.lock().unwrap() = None;
    }

    pub fn record_run(&self, run: LastRun) {
        *self.0.last_run.lock().unwrap() = Some(run);
    }

    pub fn record_push(&self, saved: serde_json::Value) {
        *self.0.last_push.lock().unwrap() = Some(saved);
    }

    pub fn set_next_summary_at(&self, unix_ms: i64) {
        self.0.next_summary_at.store(unix_ms, Ordering::Relaxed);
    }
}

#[derive(serde::Serialize)]
pub struct DevStatus {
    pub now: i64,
    pub sharing_on: bool,
    /// "listening" | "failed" (another instance has the port) | "starting"
    pub capture_server: &'static str,
    pub captures_in_buffer: usize,
    pub last_capture_at: Option<i64>,
    pub accessibility: bool,
    /// "ready" | "starting" | "down" (no model / binary, or it crashed)
    pub llm: &'static str,
    pub next_summary_at: Option<i64>,
}

/// Dev tab "Clear": start from nothing. Removes this user's curr_task on the
/// backend and everything built up on this Mac for the next summary: captured
/// pages, the last summary, the Dev tab's last run / push, and app sessions
/// (the one in front now restarts from now). Returns what was cleared.
#[tauri::command]
pub async fn dev_clear(app: AppHandle) -> Result<String, String> {
    let mut cleared = Vec::new();
    if let Some(buffer) = app.try_state::<crate::buffer::Buffer>() {
        buffer.clear_all().map_err(|err| format!("buffer: {err}"))?;
        cleared.push("captured pages + last summary");
    }
    if let Some(dev) = app.try_state::<DevState>() {
        dev.forget_runs();
    }
    if let Some(path) = crate::db::get_db_path(&app) {
        let conn = rusqlite::Connection::open(path).map_err(|err| format!("app sessions: {err}"))?;
        // The tracker keeps extending the newest row, so keep it and restart it at
        // 0 s instead of deleting it out from under the tracker.
        conn.execute_batch(
            "DELETE FROM app_sessions WHERE id <> (SELECT MAX(id) FROM app_sessions);
             UPDATE app_sessions SET started_at = ended_at WHERE id = (SELECT MAX(id) FROM app_sessions);",
        )
        .map_err(|err| format!("app sessions: {err}"))?;
        cleared.push("app sessions");
    }
    match crate::api::clear_backend_task(&app).await {
        Ok(true) => cleared.push("your task on the backend"),
        Ok(false) => {}
        Err(err) => return Err(format!("cleared {}, but the backend task: {err}", cleared.join(", "))),
    }
    println!("[dev] cleared: {}", cleared.join(", "));
    Ok(cleared.join(", "))
}

#[tauri::command]
pub fn dev_status(app: AppHandle) -> DevStatus {
    let captures = app
        .try_state::<crate::buffer::Buffer>()
        .and_then(|buffer| buffer.read_all().ok())
        .unwrap_or_default();
    let next = app
        .try_state::<DevState>()
        .map(|dev| dev.0.next_summary_at.load(Ordering::Relaxed))
        .filter(|at| *at > 0);
    DevStatus {
        now: crate::buffer::now_ms(),
        sharing_on: app
            .try_state::<crate::sharing::Sharing>()
            .is_some_and(|sharing| sharing.is_on()),
        capture_server: crate::capture_server::listening_status(),
        captures_in_buffer: captures.len(),
        // Last time the extension reached the app, even if the buffer has
        // since been wiped by a summary.
        last_capture_at: captures
            .iter()
            .map(|capture| capture.captured_at)
            .chain(crate::capture_server::last_heard_at())
            .max(),
        accessibility: super::app_tracker::accessibility_granted(),
        llm: app
            .try_state::<crate::llm::Llm>()
            .map_or("down", |llm| llm.status_label()),
        next_summary_at: next,
    }
}

#[derive(serde::Serialize)]
pub struct DevCapture {
    pub captured_at: i64,
    pub host: String,
    pub path: String,
    pub title: String,
    pub text_chars: usize,
    pub preview: String,
}

/// Chrome pages in the buffer (wiped after each summary), newest first.
#[tauri::command]
pub fn dev_captures(app: AppHandle) -> Vec<DevCapture> {
    let mut captures = app
        .try_state::<crate::buffer::Buffer>()
        .and_then(|buffer| buffer.read_all().ok())
        .unwrap_or_default();
    captures.reverse();
    captures
        .into_iter()
        .map(|capture| DevCapture {
            captured_at: capture.captured_at,
            text_chars: capture.text.chars().count(),
            preview: capture.text.chars().take(200).collect(),
            host: capture.host,
            path: capture.path,
            title: capture.title,
        })
        .collect()
}

#[derive(serde::Serialize)]
pub struct DevLastRun {
    pub run: Option<LastRun>,
    /// Backend response to the last push (topic, match result), if any.
    pub push: Option<serde_json::Value>,
}

#[tauri::command]
pub fn dev_last_run(app: AppHandle) -> DevLastRun {
    let Some(dev) = app.try_state::<DevState>() else {
        return DevLastRun { run: None, push: None };
    };
    let run = dev.0.last_run.lock().unwrap().clone();
    let push = dev.0.last_push.lock().unwrap().clone();
    DevLastRun { run, push }
}
