// Topic eval: runs the app's real summarize() (prompt, cleanup, topic snapping)
// on tauri/topic-eval/cases.json against a running llama-server, and scores
// each topic. Ignored by default because it needs the model; see
// tauri/topic-eval/README.md.
//
//     TOPIC_EVAL_LABEL=baseline cargo test topic_eval -- --ignored --nocapture
//     TOPIC_EVAL_CASES=holdout.json TOPIC_EVAL_LABEL=holdout-baseline cargo test topic_eval -- --ignored --nocapture
//
// It's a child module of summarizer.rs, so it uses the same private
// SYSTEM_PROMPT and SCHEMA as production.
use super::*;

const NOW: i64 = 1_761_300_300_000;

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    apps: Vec<CaseApp>,
    pages: Vec<CasePage>,
    shareable: bool,
    good_topics: Vec<String>,
    bad_words: Vec<String>,
    unsure: bool,
}

#[derive(Deserialize)]
struct CaseApp {
    app: String,
    title: String,
    minutes: i64,
}

#[derive(Deserialize)]
struct CasePage {
    host: String,
    path: String,
    title: String,
    text: String,
}

#[derive(Serialize)]
struct Outcome {
    name: String,
    unsure: bool,
    pass: bool,
    why: String,
    shareable: bool,
    topic: String,
    focus: String,
    /// A shared summary's focus is non-empty and names no tool or site.
    focus_ok: bool,
    task_summary: String,
}

fn inputs(case: &Case) -> (Vec<Capture>, Vec<AppSession>) {
    let captures = case
        .pages
        .iter()
        .enumerate()
        .map(|(i, page)| Capture {
            captured_at: NOW - 60_000 * (case.pages.len() - i) as i64,
            host: page.host.clone(),
            path: page.path.clone(),
            title: page.title.clone(),
            text: page.text.clone(),
        })
        .collect();
    let mut start = NOW - 10 * 60_000;
    let apps = case
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let session = AppSession {
                id: i as i64 + 1,
                app_name: app.app.clone(),
                bundle_id: None,
                window_title: Some(app.title.clone()),
                document: None,
                started_at: start,
                ended_at: start + app.minutes * 60_000,
            };
            start += app.minutes * 60_000;
            session
        })
        .collect();
    (captures, apps)
}

/// Whether the outcome is right, and why not.
fn judge(case: &Case, summary: &ActivitySummary) -> (bool, String) {
    if summary.shareable != case.shareable {
        return (false, format!("shareable={} (expected {})", summary.shareable, case.shareable));
    }
    if !case.shareable {
        return (true, String::new());
    }
    let topic = summary.topic.to_lowercase();
    if let Some(bad) = case.bad_words.iter().find(|bad| topic.contains(&bad.to_lowercase())) {
        return (false, format!("topic names a tool or site: {bad}"));
    }
    if !case.good_topics.is_empty() && !case.good_topics.iter().any(|good| good.eq_ignore_ascii_case(&summary.topic)) {
        return (false, "topic not in the accepted list".into());
    }
    (true, String::new())
}

/// Whole words, so "Gitlet" doesn't count as "git" (multi-word entries match as phrases).
fn names_any(text: &str, words: &[String]) -> bool {
    let lower = text.to_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    words.iter().any(|word| {
        let word = word.to_lowercase();
        if word.contains(' ') || word.contains('.') { lower.contains(&word) } else { tokens.contains(&word.as_str()) }
    })
}

async fn ask(client: &reqwest::Client, url: &str, prompt: String) -> Result<String, String> {
    let body = json!({
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": prompt },
        ],
        "temperature": 0,
        "max_tokens": 200,
        "stream": false,
        "response_format": { "type": "json_object", "schema": &*SCHEMA },
    });
    let reply: Value = client
        .post(url)
        .json(&body)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("request failed: {err}"))?
        .json()
        .await
        .map_err(|err| format!("bad reply: {err}"))?;
    reply["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "reply had no content".into())
}

#[tokio::test]
#[ignore = "needs a llama-server; see tauri/topic-eval/README.md"]
async fn topic_eval() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../topic-eval");
    let url = std::env::var("TOPIC_EVAL_URL").unwrap_or_else(|_| "http://127.0.0.1:8095/v1/chat/completions".into());
    let label = std::env::var("TOPIC_EVAL_LABEL").unwrap_or_else(|_| "run".into());
    let file = std::env::var("TOPIC_EVAL_CASES").unwrap_or_else(|_| "cases.json".into());
    let cases: Cases = serde_json::from_str(&std::fs::read_to_string(dir.join(&file)).unwrap()).unwrap();
    let client = reqwest::Client::new();

    let mut outcomes = Vec::new();
    for case in &cases.cases {
        let (captures, apps) = inputs(case);
        let raw = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let summary = summarize(&captures, &apps, NOW, 5, |prompt| {
            let client = client.clone();
            let url = url.clone();
            let raw = raw.clone();
            async move {
                let reply = ask(&client, &url, prompt).await;
                if let Ok(text) = &reply {
                    *raw.lock().unwrap() = text.clone();
                }
                reply
            }
        })
        .await;
        // The model's own words for the activity, before it picked a label.
        let raw_reply = serde_json::from_str::<Value>(&raw.lock().unwrap()).unwrap_or_default();
        if std::env::var("TOPIC_EVAL_RAW").is_ok() {
            println!("model reply: {raw_reply}");
            println!("pushed to the backend: {}", serde_json::to_string(&crate::api::push_payload(&summary)).unwrap());
        }
        let model_activity = format!(
            "{} → {}",
            raw_reply["activity"].as_str().unwrap_or(""),
            raw_reply["topic"].as_str().unwrap_or("")
        );
        let (pass, why) = judge(case, &summary);
        let focus_ok = !case.shareable || (!summary.focus.is_empty() && !names_any(&summary.focus, &case.bad_words));
        println!(
            "{} {:28} [{:38}] {:22} {:32} {}{}",
            if case.unsure { "?" } else if pass { "✓" } else { "✗" },
            case.name,
            model_activity,
            if summary.shareable { format!("{:?}", summary.topic) } else { "(not shared)".into() },
            if summary.shareable { format!("{}{:?}", if focus_ok { "" } else { "!" }, summary.focus) } else { String::new() },
            summary.task_summary,
            if why.is_empty() { String::new() } else { format!("   <- {why}") },
        );
        outcomes.push(Outcome {
            name: case.name.clone(),
            unsure: case.unsure,
            pass,
            why,
            shareable: summary.shareable,
            topic: summary.topic.clone(),
            focus: summary.focus.clone(),
            focus_ok,
            task_summary: summary.task_summary.clone(),
        });
    }
    let counted: Vec<&Outcome> = outcomes.iter().filter(|o| !o.unsure).collect();
    let passed = counted.iter().filter(|o| o.pass).count();
    let tool_topics = counted.iter().filter(|o| o.why.starts_with("topic names a tool")).count();
    let focus_good = counted.iter().filter(|o| o.focus_ok).count();
    println!(
        "\n{label}: {passed}/{} right ({:.0}%), {tool_topics} topics named a tool or site, focus usable in {focus_good}/{}, {} unsure not counted",
        counted.len(),
        100.0 * passed as f64 / counted.len() as f64,
        counted.len(),
        outcomes.len() - counted.len()
    );
    std::fs::write(dir.join(format!("results-{label}.json")), serde_json::to_string_pretty(&outcomes).unwrap()).unwrap();
}
