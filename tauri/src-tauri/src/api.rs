// Hook between the summarizer and the backend. Only {task_summary, topic}
// ever leaves the laptop, and only when the summary is shareable.
use crate::summarizer::ActivitySummary;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Exactly the body of the backend push (see PUSH in backend/schema.sql).
#[derive(Debug, PartialEq, Serialize)]
pub struct PushPayload<'a> {
    pub task_summary: &'a str,
    pub topic: &'a str,
}

/// None means nothing is pushed: the backend's curr_task row expires after 11
/// minutes without a push, so the user simply stops being matchable.
pub fn push_payload(summary: &ActivitySummary) -> Option<PushPayload<'_>> {
    summary.shareable.then(|| PushPayload {
        task_summary: &summary.task_summary,
        topic: &summary.topic,
    })
}

/// Push a shareable summary to the backend (POST /users/{id}/push in
/// backend/fastapi-sultan). The backend saves it as curr_task, lights up
/// matching work friends and runs the match search; a match then reaches the
/// popup through the 3 s poll in match_popup.rs.
pub fn on_summary(app: &AppHandle, summary: &ActivitySummary) {
    println!(
        "[api] summary source={:?} shareable={} topic={:?}",
        summary.source, summary.shareable, summary.topic
    );
    if crate::collecting_data::debug_logs() {
        println!("[api] summary task_summary={:?}", summary.task_summary);
    }
    let Some(payload) = push_payload(summary) else {
        return;
    };
    let Some((api_url, user_id)) = crate::match_popup::backend_target(app) else {
        println!("[api] not pushed: the main window hasn't loaded the user yet");
        return;
    };
    let Ok(body) = serde_json::to_value(&payload) else {
        return;
    };
    let url = format!("{api_url}/users/{user_id}/push");
    if crate::collecting_data::debug_logs() {
        println!("[api] POST {url} {body}");
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            reqwest::Client::new()
                .post(&url)
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json::<serde_json::Value>()
                .await
        }
        .await;
        match result {
            Ok(saved) => {
                println!(
                    "[api] pushed topic={} match={} connections={}",
                    saved["topic"], saved["match"]["match_id"], saved["connection_topics"]
                );
                if crate::collecting_data::debug_logs() {
                    println!("[api] backend match result: {}", saved["match"]);
                }
                // The main window shows what was shared and the match result.
                let _ = app.emit_to(crate::MAIN_LABEL, "activity-pushed", saved);
            }
            Err(err) => eprintln!("[api] push failed: {err}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summarizer::Source;

    fn summary(shareable: bool, source: Source) -> ActivitySummary {
        ActivitySummary {
            shareable,
            topic: if shareable { "LeetCode".into() } else { String::new() },
            task_summary: if shareable { "Working through graph problems on LeetCode".into() } else { String::new() },
            source,
            window_minutes: 5,
            created_at: 1_761_300_300_000,
        }
    }

    #[test]
    fn only_shareable_summaries_are_pushed() {
        assert_eq!(push_payload(&summary(false, Source::Llama)), None);
        assert_eq!(push_payload(&summary(false, Source::Fallback)), None);
        assert_eq!(push_payload(&summary(false, Source::Idle)), None);
        assert!(push_payload(&summary(true, Source::Llama)).is_some());
    }

    #[test]
    fn payload_is_only_task_summary_and_topic() {
        let shareable = summary(true, Source::Llama);
        let payload = serde_json::to_value(push_payload(&shareable).unwrap()).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "task_summary": "Working through graph problems on LeetCode",
                "topic": "LeetCode",
            })
        );
    }
}
