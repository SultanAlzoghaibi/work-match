// Everything that records what the user is doing on this Mac.
//
// - app_tracker: which app and window is in front, kept as a history in
//   work_match.db (app_sessions).
// - dev: read-only views for the app's Dev tab (status, captures, last run).
// - screen_capture: reads the front window with the wm-ocr sidecar and adds
//   its text to the activity buffer as source "screen".
//
// Chrome page capture (capture_server.rs, buffer.rs) and the 5-minute
// summary (summarizer.rs) live in their own files at the crate root.
pub mod app_tracker;
pub mod dev;
pub mod screen_capture;

/// Verbose activity logs (what's tracked, what the summary read, what was
/// pushed). On in dev builds; in a release build set ACTIVITY_DEBUG=1.
pub fn debug_logs() -> bool {
    cfg!(debug_assertions) || std::env::var("ACTIVITY_DEBUG").is_ok_and(|value| value == "1")
}
