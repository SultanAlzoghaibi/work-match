// Everything that records what the user is doing on this Mac.
//
// - app_tracker: which app and window is in front, kept as a history in
//   work_match.db (app_sessions).
//
// Chrome page capture (capture_server.rs, buffer.rs) and the 5-minute
// summary (summarizer.rs) live in their own files at the crate root.
pub mod app_tracker;
