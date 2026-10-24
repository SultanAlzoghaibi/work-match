// Incoming matches. Polls the FastAPI backend (backend/fastapi-sultan) every
// 3 s, shows the liquid glass popup when a new match appears, and pushes every
// change (accepted / declined / expired) to the popup page as "match-state".
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager, Runtime};

const POLL_EVERY: Duration = Duration::from_secs(3);

/// One match as GET /users/{id}/poll returns it (MatchView in
/// backend/fastapi-sultan/app/schemas.py). Only `match_id` and the statuses
/// matter here; everything else passes through to the popup unchanged.
#[derive(serde::Deserialize, serde::Serialize, Clone, PartialEq, Debug)]
pub struct MatchView {
    pub match_id: i64,
    pub my_status: String,
    pub expired: bool,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone)]
struct PollConfig {
    user_id: String,
    api_url: String,
}

#[derive(Default)]
pub struct MatchState {
    config: Mutex<Option<PollConfig>>,
    /// The match the popup is showing (None = no popup).
    current: Mutex<Option<MatchView>>,
    /// Matches the user closed; polling never reopens them.
    dismissed: Mutex<HashSet<i64>>,
}

fn show_state<R: Runtime>(app: &tauri::AppHandle<R>, view: MatchView) {
    *app.state::<MatchState>().current.lock().unwrap() = Some(view.clone());
    let _ = app.emit_to(crate::MATCH_LABEL, "match-state", view);
}

/// Apply one poll result: the newest match the user hasn't closed wins.
fn on_poll<R: Runtime>(app: &tauri::AppHandle<R>, matches: Vec<MatchView>) {
    let state = app.state::<MatchState>();
    let newest = {
        let dismissed = state.dismissed.lock().unwrap();
        matches.into_iter().find(|view| !dismissed.contains(&view.match_id))
    };
    let Some(view) = newest else {
        return;
    };
    let current = state.current.lock().unwrap().clone();
    if current.as_ref() == Some(&view) {
        return;
    }
    let is_new = current.map(|shown| shown.match_id) != Some(view.match_id);
    let answerable = view.my_status == "pending" && !view.expired;
    show_state(app, view);
    if is_new && answerable {
        crate::window::show_match_popup(app);
    }
}

/// Called once by the main window on load: only the frontend knows which
/// mock user this instance is (VITE_USER_ID) and where the API is.
#[tauri::command]
pub fn start_match_polling<R: Runtime>(app: tauri::AppHandle<R>, user_id: String, api_url: String) {
    let state = app.state::<MatchState>();
    let already_running = state
        .config
        .lock()
        .unwrap()
        .replace(PollConfig { user_id, api_url })
        .is_some();
    if already_running {
        return; // page reload: the loop picks up the new config
    }

    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::new();
        let mut last_error: Option<String> = None;
        loop {
            let config = app.state::<MatchState>().config.lock().unwrap().clone();
            if let Some(config) = config {
                let url = format!("{}/users/{}/poll", config.api_url, config.user_id);
                let result = async {
                    client.get(&url).send().await?.error_for_status()?.json::<Vec<MatchView>>().await
                }
                .await;
                match result {
                    Ok(matches) => {
                        last_error = None;
                        on_poll(&app, matches);
                    }
                    Err(err) => {
                        // Log once per distinct error, not every 3 s.
                        let message = err.to_string();
                        if last_error.as_deref() != Some(message.as_str()) {
                            eprintln!("[match] poll failed: {message}");
                            last_error = Some(message);
                        }
                    }
                }
            }
            tokio::time::sleep(POLL_EVERY).await;
        }
    });
}

/// Work together / Not now. Returns the updated match, or an error message
/// the popup shows (e.g. "this match can no longer be answered").
#[tauri::command]
pub async fn respond_match<R: Runtime>(
    app: tauri::AppHandle<R>,
    match_id: i64,
    accept: bool,
) -> Result<MatchView, String> {
    if match_id < 0 {
        return Ok(respond_mock(&app, accept));
    }
    let config = app
        .state::<MatchState>()
        .config
        .lock()
        .unwrap()
        .clone()
        .ok_or("match polling hasn't started")?;
    let url = format!(
        "{}/users/{}/matches/{}/respond",
        config.api_url, config.user_id, match_id
    );
    let response = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "accept": accept }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let detail = response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|body| body["detail"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "request failed".into());
        return Err(detail);
    }
    let view = response.json::<MatchView>().await.map_err(|err| err.to_string())?;
    show_state(&app, view.clone());
    Ok(view)
}

/// Close: hide the popup and never reopen this match.
#[tauri::command]
pub fn dismiss_match<R: Runtime>(app: tauri::AppHandle<R>, match_id: i64) {
    let state = app.state::<MatchState>();
    state.dismissed.lock().unwrap().insert(match_id);
    *state.current.lock().unwrap() = None;
    crate::window::hide_match_popup(&app);
}

/// The popup page asks for this on load, so a match raised before the page
/// finished loading isn't lost.
#[tauri::command]
pub fn current_match(state: tauri::State<MatchState>) -> Option<MatchView> {
    state.current.lock().unwrap().clone()
}

// --- Mock ("Trigger match found" button) ---------------------------------------

/// Mock matches use negative ids so respond_match never sends them to the API.
const MOCK_MATCH_ID: i64 = -1;

fn mock_view(my_status: &str) -> MatchView {
    let rest = serde_json::json!({
        "topic": "LeetCode",
        "score": 0.91,
        "created_at": null,
        "their_status": "pending",
        "other": {
            "id": null,
            "name": "Sara",
            "school": "University of Calgary",
            "bio": "CS student, prepping for internship interviews",
            "age_group": "18_24",
            "country": "CA",
            "discord_handle": "sara#0042",
            "task_summary": "Working through LeetCode dynamic programming problems, currently on longest increasing subsequence.",
        },
    });
    MatchView {
        match_id: MOCK_MATCH_ID,
        my_status: my_status.into(),
        expired: false,
        rest: rest.as_object().cloned().unwrap_or_default(),
    }
}

fn respond_mock<R: Runtime>(app: &tauri::AppHandle<R>, accept: bool) -> MatchView {
    let view = mock_view(if accept { "accepted" } else { "declined" });
    show_state(app, view.clone());
    view
}

#[tauri::command]
pub fn trigger_mock_match<R: Runtime>(app: tauri::AppHandle<R>) {
    app.state::<MatchState>()
        .dismissed
        .lock()
        .unwrap()
        .remove(&MOCK_MATCH_ID);
    show_state(&app, mock_view("pending"));
    crate::window::show_match_popup(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real GET /users/{id}/poll item from backend/fastapi-sultan.
    const POLL_ITEM: &str = r#"{"match_id": 3, "topic": "LeetCode", "score": 0.94, "created_at": "2026-10-24T09:50:42.081383Z", "my_status": "pending", "their_status": "pending", "expired": false, "other": {"id": "00000000-0000-0000-0000-000000000000", "name": "New User", "school": null, "bio": null, "age_group": null, "country": null, "discord_handle": null, "task_summary": "Grinding LeetCode dynamic programming problems."}}"#;

    #[test]
    fn poll_item_round_trips_for_the_popup() {
        let view: MatchView = serde_json::from_str(POLL_ITEM).unwrap();
        assert_eq!(view.match_id, 3);
        assert_eq!(view.my_status, "pending");
        assert!(!view.expired);
        // Everything the popup reads passes through unchanged.
        let original: serde_json::Value = serde_json::from_str(POLL_ITEM).unwrap();
        assert_eq!(serde_json::to_value(&view).unwrap(), original);
    }
}
