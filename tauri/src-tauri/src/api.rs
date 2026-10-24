// Hook between the summarizer and the backend. Only {task_summary, topic}
// ever leaves the laptop, and only when the summary is shareable.
use crate::summarizer::ActivitySummary;
use serde::Serialize;
use tauri::AppHandle;

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

pub fn on_summary(_app: &AppHandle, summary: &ActivitySummary) {
    println!(
        "[api] summary source={:?} shareable={} topic={:?}",
        summary.source, summary.shareable, summary.topic
    );
    if let Some(_payload) = push_payload(summary) {
        // TODO: push {task_summary, topic} to backend
    }
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
