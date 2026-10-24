// Turns the buffered page text into the label the backend stores in curr_task:
// a short `topic` and a one-sentence `task_summary` that a matched stranger
// will read. Runs the local Llama model; nothing here leaves the device.
use crate::buffer::{self, Buffer, Capture};
use crate::collecting_data::app_tracker::{self, AppSession};
use crate::collecting_data::dev::{DevState, LastRun};
use std::sync::Arc;
use crate::llm::Llm;
use crate::scrub::scrub;
use crate::sharing::Sharing;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::time::MissedTickBehavior;

const SYSTEM_PROMPT: &str = r#"You describe what a person is working on so they can be matched with a stranger doing
similar work. Read the captured page text from the last few minutes and output ONLY the
JSON object.
"topic": 1 to 3 words naming the activity, reusing common labels when they fit:
LeetCode, Job hunting, Apartment hunting, Coursework, Studying, Figma design, Writing,
or a course name like "CS 61B".
"task_summary": one sentence, at most 120 characters, that a stranger will read, e.g.
"Working through graph problems on LeetCode".
Pick the dominant activity. NEVER include names, emails, phone numbers, addresses,
prices, URLs, account details, message contents, or anything that identifies anyone.
Ignore email, messaging, banking, and health pages; never describe them.
If there is no clear shareable work activity, set "shareable" to false."#;

/// The labels SYSTEM_PROMPT suggests. Output is snapped to this spelling so a
/// topic stays identical across pushes (connections are stored per topic).
const KNOWN_TOPICS: [&str; 7] = [
    "LeetCode",
    "Job hunting",
    "Apartment hunting",
    "Coursework",
    "Studying",
    "Figma design",
    "Writing",
];

static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "type": "object",
        "properties": {
            "shareable":    { "type": "boolean" },
            "topic":        { "type": "string", "maxLength": 40 },
            "task_summary": { "type": "string", "maxLength": 120 }
        },
        "required": ["shareable", "topic", "task_summary"],
        "additionalProperties": false
    })
});
static COURSE_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z]{2,5}) ?(\d{1,3}[A-Za-z]{0,2})$").unwrap());

const MAX_PROMPT_CHARS: usize = 6000;
const MAX_CAPTURE_CHARS: usize = 800;
const MAX_TOPIC_WORDS: usize = 3;
const MAX_TOPIC_CHARS: usize = 40;
const MAX_SUMMARY_CHARS: usize = 120;
const MODEL_ATTEMPTS: usize = 2;
/// How long the first summaries wait for llama-server to finish loading.
const READY_WAIT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Llama,
    Fallback,
    Idle,
}

/// Emitted as `activity-summary` and stored in last_summary. Only
/// `task_summary` and `topic` are ever pushed (see api::push_payload).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivitySummary {
    pub shareable: bool,
    pub topic: String,
    pub task_summary: String,
    pub source: Source,
    pub window_minutes: u32,
    pub created_at: i64,
}

impl ActivitySummary {
    /// Not shareable always means empty fields, so rejected text is never stored or shown.
    fn not_shareable(source: Source, created_at: i64, window_minutes: u32) -> Self {
        Self {
            shareable: false,
            topic: String::new(),
            task_summary: String::new(),
            source,
            window_minutes,
            created_at,
        }
    }
}

#[derive(Deserialize)]
struct ModelOutput {
    shareable: bool,
    topic: String,
    task_summary: String,
}

pub fn start(app: AppHandle) {
    let demo = std::env::var("DEMO_MODE").is_ok_and(|value| value == "1");
    // The first summary runs soon after launch, so a new user is matchable within a minute.
    let (first_run, every, window_minutes) = if demo {
        (Duration::from_secs(30), Duration::from_secs(30), 1)
    } else {
        (Duration::from_secs(60), Duration::from_secs(5 * 60), 5)
    };
    if demo {
        println!("[summarizer] demo mode: summarizing every 30 s");
    }
    let buffer = app.state::<Buffer>().inner().clone();
    let sharing = app.state::<Sharing>().inner().clone();
    let llm = app.state::<Llm>().inner().clone();
    let dev = app.state::<DevState>().inner().clone();
    let db_path = crate::db::get_db_path(&app);

    tauri::async_runtime::spawn(async move {
        // The topic is remembered with the sharing epoch it was learned in, so a
        // pause forgets it along with the buffer.
        let mut previous_topic = last_shareable_topic(&buffer).map(|topic| (sharing.epoch(), topic));
        dev.set_next_summary_at(buffer::now_ms() + first_run.as_millis() as i64);
        tokio::time::sleep(first_run).await;
        let mut ticker = tokio::time::interval(every);
        // After the laptop sleeps, run once on wake instead of catching up on every missed tick.
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            dev.set_next_summary_at(buffer::now_ms() + every.as_millis() as i64);
            let apps = recent_apps(db_path.as_deref(), window_minutes);
            if let Some(summary) =
                run_once(&buffer, &sharing, &llm, &dev, &apps, &mut previous_topic, window_minutes).await
            {
                let _ = app.emit("activity-summary", &summary);
                crate::api::on_summary(&app, &summary);
            }
        }
    });
}

/// Apps in front during the last window (collecting_data/app_tracker.rs).
fn recent_apps(db_path: Option<&std::path::Path>, window_minutes: u32) -> Vec<AppSession> {
    let since = buffer::now_ms() - i64::from(window_minutes) * 60_000;
    db_path
        .map(|path| app_tracker::recent_sessions(path, since))
        .unwrap_or_default()
}

/// "Push now" in the app: summarize the last 5 minutes right away instead of
/// waiting for the timer, then push it like a scheduled summary would.
#[tauri::command]
pub async fn summarize_now(app: AppHandle) -> Result<ActivitySummary, String> {
    let buffer = app.state::<Buffer>().inner().clone();
    let sharing = app.state::<Sharing>().inner().clone();
    let llm = app.state::<Llm>().inner().clone();
    let dev = app.state::<DevState>().inner().clone();
    if !sharing.is_on() {
        return Err("Share my activity is off: turn it on in the tray menu".into());
    }
    const WINDOW_MINUTES: u32 = 5;
    println!("[summarizer] summarize now (requested from the app)");
    let mut previous_topic = last_shareable_topic(&buffer).map(|topic| (sharing.epoch(), topic));
    let apps = recent_apps(crate::db::get_db_path(&app).as_deref(), WINDOW_MINUTES);
    let summary = run_once(&buffer, &sharing, &llm, &dev, &apps, &mut previous_topic, WINDOW_MINUTES)
        .await
        .ok_or("sharing was turned off while summarizing")?;
    let _ = app.emit("activity-summary", &summary);
    crate::api::on_summary(&app, &summary);
    Ok(summary)
}

/// One summary of everything buffered, or None if sharing is off or was
/// paused while the model ran.
async fn run_once(
    buffer: &Buffer,
    sharing: &Sharing,
    llm: &Llm,
    dev: &DevState,
    apps: &[AppSession],
    previous_topic: &mut Option<(u64, String)>,
    window_minutes: u32,
) -> Option<ActivitySummary> {
    if !sharing.is_on() {
        return None;
    }
    let epoch = sharing.epoch();
    let captures = match buffer.read_all() {
        Ok(captures) => captures,
        Err(err) => {
            eprintln!("[summarizer] reading the buffer failed: {err}");
            return None;
        }
    };
    if !captures.is_empty() || !apps.is_empty() {
        // If it isn't ready in time, chat() fails fast and the summary falls back.
        llm.wait_ready(READY_WAIT).await;
    }
    let hint = previous_topic
        .as_ref()
        .filter(|(learned_in, _)| *learned_in == epoch)
        .map(|(_, topic)| topic.as_str());
    // Record what the model read and replied, for the Dev tab.
    let seen = Arc::new(std::sync::Mutex::new((None::<String>, Vec::<String>::new())));
    let started_at = buffer::now_ms();
    let summary = summarize(&captures, apps, hint, started_at, window_minutes, |prompt| {
        let llm = llm.clone();
        let seen = seen.clone();
        seen.lock().unwrap().0 = Some(prompt.clone());
        async move {
            let reply = llm.chat(SYSTEM_PROMPT, &prompt, &SCHEMA).await;
            seen.lock().unwrap().1.push(match &reply {
                Ok(text) => text.clone(),
                Err(err) => format!("error: {err}"),
            });
            reply
        }
    })
    .await;
    let (prompt, replies) = seen.lock().unwrap().clone();
    dev.record_run(LastRun {
        at: started_at,
        prompt,
        replies,
        summary: serde_json::to_value(&summary).unwrap_or_default(),
    });

    if let Some(max_id) = captures.iter().map(|capture| capture.id).max() {
        // On failure the rows are read again next time, and swept within 6 minutes regardless.
        if let Err(err) = buffer.delete_through(max_id) {
            eprintln!("[summarizer] deleting summarized rows failed: {err}");
        }
    }
    if !sharing.is_on() || sharing.epoch() != epoch {
        return None;
    }
    if summary.shareable {
        *previous_topic = Some((epoch, summary.topic.clone()));
    }
    let saved = serde_json::to_string(&summary)
        .map_err(|err| err.to_string())
        .and_then(|json| buffer.save_last_summary(summary.created_at, &json).map_err(|err| err.to_string()));
    if let Err(err) = saved {
        eprintln!("[summarizer] saving last_summary failed: {err}");
    }
    Some(summary)
}

/// No captures and no app sessions → idle without calling the model. Otherwise the model gets two
/// attempts; a transport error or unparseable reply counts as a failure, and
/// two failures give the fallback.
// `ask_model` takes the prompt by value and returns an owned future (rather
// than being an async closure over &str) so the summarizer task stays Send.
pub async fn summarize<F, Fut>(
    captures: &[Capture],
    apps: &[AppSession],
    previous_topic: Option<&str>,
    now: i64,
    window_minutes: u32,
    mut ask_model: F,
) -> ActivitySummary
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    if captures.is_empty() && apps.is_empty() {
        return ActivitySummary::not_shareable(Source::Idle, now, window_minutes);
    }
    let prompt = build_prompt(captures, apps, previous_topic);
    if crate::collecting_data::debug_logs() {
        println!(
            "[summarizer] ---- what the model reads ({} pages, {} app sessions) ----\n{prompt}\n[summarizer] ---- end ----",
            captures.len(),
            apps.len()
        );
    }
    for attempt in 1..=MODEL_ATTEMPTS {
        let output = ask_model(prompt.clone()).await.and_then(|reply| {
            serde_json::from_str::<ModelOutput>(&reply).map_err(|err| format!("bad JSON from the model: {err}"))
        });
        match output {
            Ok(output) => {
                return match clean(output) {
                    Some((topic, task_summary)) => ActivitySummary {
                        shareable: true,
                        topic,
                        task_summary,
                        source: Source::Llama,
                        window_minutes,
                        created_at: now,
                    },
                    None => ActivitySummary::not_shareable(Source::Llama, now, window_minutes),
                }
            }
            Err(err) => eprintln!("[summarizer] attempt {attempt}/{MODEL_ATTEMPTS} failed: {err}"),
        }
    }
    ActivitySummary::not_shareable(Source::Fallback, now, window_minutes)
}

/// Apps in front (with time spent) first, then pages newest first, one entry
/// per page (its latest text), within MAX_PROMPT_CHARS.
fn build_prompt(captures: &[Capture], apps: &[AppSession], previous_topic: Option<&str>) -> String {
    let mut prompt = String::new();
    if let Some(topic) = previous_topic {
        prompt.push_str(&format!(
            "Previous topic: {topic} (reuse this exact label if the activity is the same)\n\n"
        ));
    }
    prompt.push_str(&apps_section(apps));
    if captures.is_empty() {
        return prompt;
    }
    prompt.push_str("Pages from the last few minutes, newest first:\n");
    let mut seen = HashSet::new();
    let mut used = 0;
    for capture in captures.iter().rev() {
        if !seen.insert((capture.host.as_str(), capture.path.as_str())) {
            continue;
        }
        let text: String = capture.text.chars().take(MAX_CAPTURE_CHARS).collect();
        let entry = format!(
            "\n[{}] {}{} | {}\n{}\n",
            clock_time(capture.captured_at),
            capture.host,
            capture.path.trim_end_matches('/'),
            capture.title,
            text
        );
        let entry_chars = entry.chars().count();
        if used > 0 && used + entry_chars > MAX_PROMPT_CHARS {
            break;
        }
        used += entry_chars;
        prompt.push_str(&entry);
    }
    prompt
}

const MAX_APPS_CHARS: usize = 1500;

/// "Apps in front in the last few minutes" lines, longest first, with repeats
/// of the same app + window + document merged.
fn apps_section(apps: &[AppSession]) -> String {
    let mut merged: Vec<(String, i64)> = Vec::new();
    for session in apps {
        let mut line = session.app_name.clone();
        if let Some(title) = &session.window_title {
            line.push_str(&format!(": {title}"));
        }
        if let Some(document) = &session.document {
            line.push_str(&format!(" [{document}]"));
        }
        let millis = (session.ended_at - session.started_at).max(0);
        match merged.iter_mut().find(|(existing, _)| *existing == line) {
            Some((_, total)) => *total += millis,
            None => merged.push((line, millis)),
        }
    }
    if merged.is_empty() {
        return String::new();
    }
    merged.sort_by(|a, b| b.1.cmp(&a.1));
    let mut section = String::from("Apps in front in the last few minutes, most time first:\n");
    for (line, millis) in merged {
        let entry = format!("- {} ({})\n", line, duration_label(millis));
        if section.chars().count() + entry.chars().count() > MAX_APPS_CHARS {
            break;
        }
        section.push_str(&entry);
    }
    section.push('\n');
    section
}

fn duration_label(millis: i64) -> String {
    let seconds = millis / 1000;
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", (seconds + 30) / 60)
    }
}

fn clock_time(unix_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .map(|time| time.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// task_summary is shown to a stranger, so this is the last gate: both fields
/// are scrubbed again, and anything that still looks like contact details,
/// links or money makes the whole summary not shareable.
fn clean(output: ModelOutput) -> Option<(String, String)> {
    if !output.shareable {
        return None;
    }
    let topic = scrub(&output.topic);
    let task_summary = scrub(&output.task_summary);
    if looks_identifying(&topic) || looks_identifying(&task_summary) {
        return None;
    }
    let task_summary = cut_at_word(&task_summary, MAX_SUMMARY_CHARS);
    let topic = canonical_topic(&first_words(&topic, MAX_TOPIC_WORDS, MAX_TOPIC_CHARS), &task_summary);
    (!topic.is_empty() && !task_summary.is_empty()).then_some((topic, task_summary))
}

fn looks_identifying(field: &str) -> bool {
    let field = field.to_lowercase();
    ["[email]", "[phone]", "[number]", "@", "$", "http://", "https://", "www."]
        .iter()
        .any(|marker| field.contains(marker))
}

fn first_words(text: &str, words: usize, max_chars: usize) -> String {
    let joined = text.split_whitespace().take(words).collect::<Vec<_>>().join(" ");
    joined.chars().take(max_chars).collect::<String>().trim_end().to_string()
}

fn cut_at_word(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    match cut.rfind(' ') {
        Some(space) if space > 0 => cut[..space].trim_end().to_string(),
        _ => cut,
    }
}

fn canonical_topic(topic: &str, task_summary: &str) -> String {
    if let Some(known) = KNOWN_TOPICS.iter().find(|known| known.eq_ignore_ascii_case(topic)) {
        return known.to_string();
    }
    if let Some(course) = COURSE_CODE.captures(topic) {
        return format!("{} {}", course[1].to_uppercase(), course[2].to_uppercase());
    }
    // Llama tends to name the problem ("Course Schedule") rather than the site.
    // Connections are stored per topic, so all of LeetCode has to be one topic.
    if task_summary.to_lowercase().contains("leetcode") {
        return "LeetCode".to_string();
    }
    topic.to_string()
}

fn last_shareable_topic(buffer: &Buffer) -> Option<String> {
    let json = buffer.last_summary().ok()??;
    let summary: ActivitySummary = serde_json::from_str(&json).ok()?;
    summary.shareable.then_some(summary.topic)
}

#[tauri::command]
pub fn get_last_summary(buffer: State<'_, Buffer>) -> Result<Option<ActivitySummary>, String> {
    let Some(json) = buffer.last_summary().map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    serde_json::from_str(&json).map(Some).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_761_300_300_000;

    fn capture(id: i64, host: &str, path: &str, title: &str, text: &str) -> Capture {
        Capture {
            id,
            captured_at: NOW - 60_000 + id * 1_000,
            host: host.into(),
            path: path.into(),
            title: title.into(),
            text: text.into(),
        }
    }

    fn leetcode() -> Vec<Capture> {
        vec![capture(1, "leetcode.com", "/problems/course-schedule/", "Course Schedule - LeetCode", "There are numCourses courses")]
    }

    fn output(shareable: bool, topic: &str, task_summary: &str) -> ModelOutput {
        ModelOutput {
            shareable,
            topic: topic.into(),
            task_summary: task_summary.into(),
        }
    }

    fn reply(topic: &str, task_summary: &str) -> String {
        json!({ "shareable": true, "topic": topic, "task_summary": task_summary }).to_string()
    }

    #[test]
    fn clean_keeps_a_good_label() {
        assert_eq!(
            clean(output(true, "leetcode", " Working through graph problems on LeetCode ")),
            Some(("LeetCode".into(), "Working through graph problems on LeetCode".into()))
        );
    }

    #[test]
    fn clean_rejects_anything_identifying() {
        for summary in [
            "Writing to jane@example.com about a job",
            "Calling (415) 555-1234 about a lease",
            "Tracking order 123456789",
            "Reading https://example.com/docs",
            "Browsing www.example.com listings",
            "Chatting with @sam about rent",
            "Apartments under $2,000 in Oakland",
        ] {
            assert_eq!(clean(output(true, "Apartment hunting", summary)), None, "{summary}");
        }
        assert_eq!(clean(output(true, "jane@example.com", "Working on something")), None);
    }

    #[test]
    fn clean_refuses_unshareable_or_empty_output() {
        assert_eq!(clean(output(false, "Email", "Reading email")), None);
        assert_eq!(clean(output(true, "  ", "Working on something")), None);
        assert_eq!(clean(output(true, "LeetCode", " \n ")), None);
    }

    #[test]
    fn clean_allows_http_as_a_word() {
        assert!(clean(output(true, "Web development", "Learning HTTP caching for a backend")).is_some());
    }

    #[test]
    fn clean_truncates_long_fields() {
        let long_summary = "Working through graph problems ".repeat(10);
        let (topic, task_summary) =
            clean(output(true, "graph algorithms practice for interviews", &long_summary)).unwrap();
        assert_eq!(topic, "graph algorithms practice");
        assert!(task_summary.chars().count() <= MAX_SUMMARY_CHARS);
        assert!(task_summary.starts_with("Working through graph problems"));
        assert!(!task_summary.ends_with(' '));
        assert!(long_summary.contains(&format!("{task_summary} ")), "cut mid-word: {task_summary}");
    }

    #[test]
    fn topics_use_one_spelling() {
        let summary = "Working on something";
        assert_eq!(canonical_topic("JOB HUNTING", summary), "Job hunting");
        assert_eq!(canonical_topic("leetcode", summary), "LeetCode");
        assert_eq!(canonical_topic("cs61b", summary), "CS 61B");
        assert_eq!(canonical_topic("Cs 170", summary), "CS 170");
        assert_eq!(canonical_topic("Graph algorithms", summary), "Graph algorithms");
    }

    #[test]
    fn leetcode_problems_share_one_topic() {
        // Real 3B output for the Course Schedule page.
        assert_eq!(
            canonical_topic("Course Schedule", "Working through graph problems on LeetCode"),
            "LeetCode"
        );
        assert_eq!(canonical_topic("CS 170", "Practicing CS 170 problems on LeetCode"), "CS 170");
        assert_eq!(canonical_topic("Gitlet design", "Designing Gitlet classes"), "Gitlet design");
    }

    #[tokio::test]
    async fn no_captures_is_idle_without_calling_the_model() {
        let mut calls = 0;
        let summary = summarize(&[], &[], None, NOW, 5, |_| {
            calls += 1;
            async { Err("unused".to_string()) }
        })
        .await;
        assert_eq!(summary, ActivitySummary::not_shareable(Source::Idle, NOW, 5));
        assert_eq!(calls, 0);
    }

    #[tokio::test]
    async fn falls_back_after_a_failed_retry() {
        let mut calls = 0;
        let summary = summarize(&leetcode(), &[], None, NOW, 5, |_| {
            calls += 1;
            async { Err("connection refused".to_string()) }
        })
        .await;
        assert_eq!(calls, 2);
        assert_eq!(summary, ActivitySummary::not_shareable(Source::Fallback, NOW, 5));
    }

    #[tokio::test]
    async fn bad_json_twice_falls_back() {
        let mut calls = 0;
        let summary = summarize(&leetcode(), &[], None, NOW, 5, |_| {
            calls += 1;
            async { Ok("{\"topic\": \"LeetCode\"".to_string()) }
        })
        .await;
        assert_eq!(calls, 2);
        assert_eq!(summary.source, Source::Fallback);
    }

    #[tokio::test]
    async fn succeeds_on_the_retry() {
        let mut calls = 0;
        let summary = summarize(&leetcode(), &[], None, NOW, 5, |prompt| {
            calls += 1;
            let first_try = calls == 1;
            async move {
                assert!(prompt.contains("leetcode.com/problems/course-schedule"));
                if first_try {
                    Err("timeout".to_string())
                } else {
                    Ok(reply("LeetCode", "Working through graph problems on LeetCode"))
                }
            }
        })
        .await;
        assert_eq!(calls, 2);
        assert!(summary.shareable);
        assert_eq!(summary.source, Source::Llama);
        assert_eq!(summary.topic, "LeetCode");
    }

    #[tokio::test]
    async fn rejected_output_is_blank_and_not_shareable() {
        let summary = summarize(&leetcode(), &[], None, NOW, 5, |_| async {
            Ok(reply("Job hunting", "Emailing recruiter jane@example.com"))
        })
        .await;
        assert_eq!(summary, ActivitySummary::not_shareable(Source::Llama, NOW, 5));
    }

    #[test]
    fn prompt_is_newest_first_one_entry_per_page_and_capped() {
        let captures = vec![
            capture(1, "leetcode.com", "/problems/course-schedule/", "old title", "old text"),
            capture(2, "www.zillow.com", "/oakland-ca/rentals/", "Oakland Rentals", &"z".repeat(5_000)),
            capture(3, "leetcode.com", "/problems/course-schedule/", "Course Schedule - LeetCode", "new text"),
        ];
        let prompt = build_prompt(&captures, &[], Some("LeetCode"));

        assert!(prompt.starts_with("Previous topic: LeetCode"));
        let leetcode_at = prompt.find("leetcode.com/problems/course-schedule | Course Schedule").unwrap();
        let zillow_at = prompt.find("www.zillow.com/oakland-ca/rentals | Oakland Rentals").unwrap();
        assert!(leetcode_at < zillow_at, "newest first");
        assert!(prompt.contains("new text"));
        assert!(!prompt.contains("old text"), "one entry per page");
        assert!(!prompt.contains(&"z".repeat(MAX_CAPTURE_CHARS + 1)));
        assert!(prompt.contains(&"z".repeat(MAX_CAPTURE_CHARS)));

        let many: Vec<_> = (0..20)
            .map(|id| capture(id, "example.com", &format!("/page/{id}"), "Page", &"x".repeat(800)))
            .collect();
        assert!(build_prompt(&many, &[], None).chars().count() < MAX_PROMPT_CHARS + 100);
    }

    fn app(name: &str, title: &str, started_at: i64, ended_at: i64) -> AppSession {
        AppSession {
            id: started_at,
            app_name: name.into(),
            bundle_id: None,
            window_title: Some(title.into()),
            document: None,
            started_at,
            ended_at,
        }
    }

    #[test]
    fn apps_section_merges_repeats_and_puts_most_time_first() {
        let apps = [
            app("Chrome", "Two Sum - LeetCode", 0, 60_000),
            app("Code", "dijkstra.py", 60_000, 300_000),
            app("Chrome", "Two Sum - LeetCode", 300_000, 360_000),
        ];
        let prompt = build_prompt(&[], &apps, None);
        let code = prompt.find("Code: dijkstra.py (4 min)").expect("code line");
        let chrome = prompt.find("Chrome: Two Sum - LeetCode (2 min)").expect("merged chrome line");
        assert!(code < chrome);
        assert!(!prompt.contains("Pages from"));
    }

    #[tokio::test]
    async fn app_sessions_alone_are_summarized_not_idle() {
        let apps = [app("Code", "dijkstra.py", 0, 300_000)];
        let summary = summarize(&[], &apps, None, NOW, 5, |_| async {
            Ok(reply("Coursework", "Implementing Dijkstra's algorithm"))
        })
        .await;
        assert_eq!(summary.source, Source::Llama);
        assert!(summary.shareable);
    }

}
