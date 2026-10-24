// Incoming match requests. For now a mock offer is raised from the main
// panel's "Trigger match found" button; later the /poll loop calls
// `offer_match` with real data from the backend.
use std::sync::Mutex;
use tauri::{Emitter, Runtime};

#[derive(serde::Serialize, Clone)]
pub struct MatchOffer {
    pub match_id: i64,
    pub topic: String,
    pub other_name: String,
    pub school: Option<String>,
    pub bio: Option<String>,
    pub task_summary: String,
}

/// The offer the popup is currently showing (None = no popup).
#[derive(Default)]
pub struct PendingMatch(pub Mutex<Option<MatchOffer>>);

/// Store the offer, tell the popup page to render it, and show the popup.
pub fn offer_match<R: Runtime>(app: &tauri::AppHandle<R>, offer: MatchOffer) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<PendingMatch>() {
        *state.0.lock().unwrap() = Some(offer.clone());
    }
    let _ = app.emit_to(crate::MATCH_LABEL, "match-offer", offer);
    crate::window::show_match_popup(app);
}

fn mock_offer() -> MatchOffer {
    MatchOffer {
        match_id: 41,
        topic: "LeetCode".into(),
        other_name: "Sara".into(),
        school: Some("University of Calgary".into()),
        bio: Some("CS student, prepping for internship interviews".into()),
        task_summary: "Working through LeetCode dynamic programming problems, currently on \
                       longest increasing subsequence."
            .into(),
    }
}

#[tauri::command]
pub fn trigger_mock_match<R: Runtime>(app: tauri::AppHandle<R>) {
    offer_match(&app, mock_offer());
}

/// The popup page asks for this on load, so an offer raised before the page
/// finished loading isn't lost.
#[tauri::command]
pub fn current_match(state: tauri::State<PendingMatch>) -> Option<MatchOffer> {
    state.0.lock().unwrap().clone()
}

/// Work together / Not now / expired. Mock: just closes the popup.
/// TODO: POST /matches/{id}/respond once the backend exists.
#[tauri::command]
pub fn respond_match<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<PendingMatch>,
    match_id: i64,
    accept: bool,
) {
    println!("[match] respond match_id={match_id} accept={accept}");
    *state.0.lock().unwrap() = None;
    crate::window::hide_match_popup(&app);
}
