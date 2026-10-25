// Turns the buffered page text into the label the backend stores in curr_task:
// a short `topic` and a `task_summary` describing the task, which a matched stranger
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
similar work. Read the apps and pages from the last few minutes and output ONLY the JSON object.
"activity": first, in your own words, the general field or hobby this belongs to, in 1 or 2
words, as you would name a school club for it. Never the specific task, page, subject or
title, and never a tool.
"topic": then a short label (1 to 3 words) for the general kind of activity, worded the way
anyone doing the same thing would get it. These are common examples: reuse one when it really
means the same thing, otherwise write your own (for example Travel planning, Cooking, Fitness,
Language learning). Decide by what the person is doing, not by the tools, commands or apps on screen:
- Coding: writing, debugging or setting up software, including reading its docs and tutorials
- Web development: websites, web apps, frontends, backends and APIs
- App development: mobile or desktop apps
- Data science: analyzing or cleaning data, notebooks, statistics
- Machine learning: building, training or studying ML models
- Design: UI, product or graphic design; Figma design when it's in Figma
- Writing: essays, reports, theses, applications and other writing
- Research: academic research and reading papers (not reading docs while coding)
- Studying: learning or reviewing material, e.g. for an exam
- Coursework: homework, labs and projects for a class
- Music: practicing an instrument, composing, producing or mixing music
- LeetCode: coding interview practice problems
- Job hunting: looking for jobs or internships
- Apartment hunting: looking for a place to live
- Shopping: looking for things to buy
"focus": 2 to 4 words naming the specific subject inside that topic: the project,
problem, concept, course material or place they are working on. Never a tool, app,
website, command or file name, and never just the topic again.
"task_summary": a concise, truthful description of the task they are doing, for a stranger
who might work alongside them: what they are making, solving, learning or looking for, and
the specific subject. Only state what the apps and pages show; don't guess or add details.
Never copy a page title, and never name an app, website, tool, command or file, except
LeetCode and Figma.
Pick the dominant activity. NEVER include names, emails, phone numbers, addresses,
prices, URLs, account details, message contents, or anything that identifies anyone.
Ignore email, messaging, banking, and health pages; never describe them.
If there is no clear shareable work activity, set "shareable" to false."#;

/// The labels SYSTEM_PROMPT suggests. Output is snapped to this spelling so a
/// topic stays identical across pushes (connections are stored per topic).
/// Picked when nothing in KNOWN_TOPICS fits; the topic is then the model's own `activity`.
const OTHER_TOPIC: &str = "Other";

const KNOWN_TOPICS: [&str; 16] = [
    "Coding",
    "Web development",
    "App development",
    "Data science",
    "Machine learning",
    "Design",
    "Figma design",
    "Writing",
    "Research",
    "Studying",
    "Coursework",
    "Music",
    "LeetCode",
    "Job hunting",
    "Apartment hunting",
    "Shopping",
];


static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "type": "object",
        "properties": {
            "shareable":    { "type": "boolean" },
            // Written first, in the model's own words, so the list below doesn't force
            // an activity into the nearest label. Used as the topic when that's Other.
            "activity":     { "type": "string", "maxLength": 24 },
            // Free text: the prompt's list is examples, not a forced choice (a forced
            // list squeezed flights into "Job hunting"). canonical_topic snaps the
            // common ones to one spelling.
            "topic":        { "type": "string", "maxLength": 40 },
            "focus":        { "type": "string", "maxLength": 40 },
            "task_summary": { "type": "string" }
        },
        "required": ["shareable", "activity", "topic", "focus", "task_summary"],
        "additionalProperties": false
    })
});
/// A course code in a window or page title: "CS 61B", "CS70", "Bio 101", "Physics 7B".
/// Department in capitals, or a capitalized word with a 3-digit number or a letter suffix
/// (so "Note 12" and "Summer 2027" don't count).
static COURSE_IN_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:([A-Z]{2,5}) ?(\d{1,3}[A-Z]{0,2})|([A-Z][a-z]{2,9}) (\d{3}[A-Z]?|\d{1,2}[A-Z]))\b").unwrap()
});

/// With a course on screen, the course becomes the topic, unless the person is clearly
/// doing something that isn't for a class.
const NOT_COURSE_WORK: [&str; 3] = ["Job hunting", "Apartment hunting", "Shopping"];

/// LeetCode on screen + a coding or studying topic means interview practice. The model
/// is easily led by problem names ("Course Schedule" → Coursework).
fn leetcode_topic(topic: &str, captures: &[Capture], apps: &[AppSession]) -> Option<String> {
    // NeetCode is LeetCode's problems in roadmap order, so both are one topic.
    let on_leetcode = captures.iter().any(|capture| capture.host.ends_with("leetcode.com") || capture.host.ends_with("neetcode.io"))
        || apps.iter().any(|app| {
            app.window_title.as_deref().is_some_and(|title| title.contains("LeetCode") || title.contains("NeetCode"))
        });
    let coding_or_studying = ["Coding", "Web development", "App development", "Studying", "Coursework"];
    (on_leetcode && coding_or_studying.contains(&topic)).then(|| "LeetCode".to_string())
}

/// Sites whose pages are job listings (LinkedIn only under /jobs). A subdomain
/// counts: boards.greenhouse.io, jobs.lever.co, acme.myworkdayjobs.com.
const JOB_BOARDS: [&str; 11] = [
    "indeed.com", "glassdoor.com", "jobright.ai", "joinhandshake.com", "wellfound.com", "greenhouse.io",
    "lever.co", "ashbyhq.com", "myworkdayjobs.com", "ziprecruiter.com", "simplify.jobs",
];
const BROWSERS: [&str; 6] = ["Chrome", "Edge", "Brave", "Arc", "Opera", "Vivaldi"];

fn is_job_board(capture: &Capture) -> bool {
    let host = capture.host.to_lowercase();
    if host == "linkedin.com" || host.ends_with(".linkedin.com") {
        return capture.path.starts_with("/jobs");
    }
    JOB_BOARDS.iter().any(|board| host == *board || host.ends_with(&format!(".{board}")))
}

/// Mostly job listings in the browser means Job hunting, whatever the postings are
/// for: the model calls a research posting Research and writes its duties as the
/// person's task. Not when another app had more time (a glance while coding), and
/// not Writing (a cover letter next to the posting).
fn job_topic(topic: &str, captures: &[Capture], apps: &[AppSession]) -> Option<String> {
    let boards = captures.iter().filter(|capture| is_job_board(capture)).count();
    if topic == "Writing" || boards * 2 <= captures.len() {
        return None;
    }
    let time_in = |browser: bool| -> i64 {
        apps.iter()
            .filter(|app| app.app_name.split_whitespace().any(|word| BROWSERS.contains(&word)) == browser)
            .map(|app| (app.ended_at - app.started_at).max(0))
            .sum()
    };
    (time_in(true) >= time_in(false)).then(|| "Job hunting".to_string())
}

/// A screen rule that moved the topic means the model read the page as the person's
/// own work ("Designing system architecture for GPU platforms" for a job posting, a
/// problem statement for practice), so that summary is replaced unless it already
/// says what they're doing. It's what the matched stranger reads and Jev compares.
fn summary_after_screen_rule(topic: &str, focus: &str, task_summary: &str) -> Option<String> {
    let (says_it, plain) = match topic {
        "Job hunting" => (
            ["job", "role", "position", "intern", "hiring", "career"],
            if focus.is_empty() { "Browsing job listings".to_string() } else { format!("Browsing job listings for {focus} roles") },
        ),
        "LeetCode" => (
            ["leetcode", "problem", "practic", "interview", "algorithm", "data structure"],
            if focus.is_empty() { "Practicing LeetCode problems".to_string() } else { format!("Practicing LeetCode problems: {focus}") },
        ),
        _ => return None,
    };
    let summary = task_summary.to_lowercase();
    (!says_it.iter().any(|words| summary.contains(words))).then_some(plain)
}

/// Coursework or studying for a course named on screen gets the course as its topic,
/// so classmates in the same course connect on one topic.
fn course_topic(topic: &str, captures: &[Capture], apps: &[AppSession]) -> Option<String> {
    if NOT_COURSE_WORK.contains(&topic) {
        return None;
    }
    let titles = captures
        .iter()
        .map(|capture| capture.title.as_str())
        .chain(apps.iter().filter_map(|app| app.window_title.as_deref()));
    titles.into_iter().find_map(|title| {
        let found = COURSE_IN_TITLE.captures(title)?;
        // Capital departments as written ("CS 61B"); word departments keep their case ("Physics 7B").
        match (found.get(1), found.get(2), found.get(3), found.get(4)) {
            (Some(dept), Some(number), _, _) => Some(format!("{} {}", dept.as_str(), number.as_str())),
            (_, _, Some(dept), Some(number)) => Some(format!("{} {}", dept.as_str(), number.as_str())),
            _ => None,
        }
    })
}

static COURSE_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z]{2,5}) ?(\d{1,3}[A-Za-z]{0,2})$").unwrap());

/// Pages and apps the model never sees: email, messaging, banking and health.
/// The prompt also says to ignore them, but the eval (summarizer-eval/) showed
/// the model shares health pages anyway, so they're dropped here first.
/// Also work-match's own Jitsi calls (match_popup.rs) and Jitsi's sign-in page
/// (meet-jit-si-*.firebaseapp.com): a call isn't the work it was matched on.
const PRIVATE_HOSTS: [&str; 26] = [
    "mail.google.com", "outlook.live.com", "outlook.office.com", "mail.yahoo.com", "proton.me",
    "web.whatsapp.com", "messenger.com", "web.telegram.org", "discord.com", "slack.com",
    "chase.com", "bankofamerica.com", "wellsfargo.com", "capitalone.com", "paypal.com", "venmo.com",
    "rbc.com", "td.com", "mychart", "webmd.com", "healthline.com", "mayoclinic.org",
    "patient.info", "zocdoc.com", "meet.jit.si", "meet-jit-si",
];
const PRIVATE_APPS: [&str; 9] = [
    "Mail", "Messages", "Slack", "Discord", "WhatsApp", "Telegram", "Signal", "Messenger", "Outlook",
];

fn is_private_host(host: &str) -> bool {
    let host = host.to_lowercase();
    PRIVATE_HOSTS.iter().any(|private| {
        host == *private || host.ends_with(&format!(".{private}")) || (!private.contains('.') && host.contains(private))
    })
}

fn is_private_app(app: &AppSession) -> bool {
    // Whole words, so "Microsoft Outlook" (macOS name, Windows FileDescription) counts.
    PRIVATE_APPS.iter().any(|name| app.app_name.split_whitespace().any(|word| word.eq_ignore_ascii_case(name)))
        // A browser window showing a private page (its title names the site).
        || app.window_title.as_deref().is_some_and(|title| {
            let title = title.to_lowercase();
            ["gmail", "outlook", "mychart", "webmd", "online banking", "whatsapp", "jitsi meet"]
                .iter()
                .any(|marker| title.contains(marker))
        })
}

const MAX_PROMPT_CHARS: usize = 6000;
const MAX_CAPTURE_CHARS: usize = 800;
const MAX_TOPIC_WORDS: usize = 3;
const MAX_TOPIC_CHARS: usize = 40;
// The prompt asks for a concise description and gives no length; this only
// stops a runaway reply (backend TaskPush.task_summary allows up to 4000).
const MAX_SUMMARY_CHARS: usize = 400;
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
    /// The specific subject within `topic`, e.g. topic "Coding", focus "desktop matching app".
    /// Empty when the model's focus was missing or named a tool.
    #[serde(default)]
    pub focus: String,
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
            focus: String::new(),
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
    #[serde(default)]
    activity: String,
    #[serde(default)]
    focus: String,
    task_summary: String,
}

pub fn start(app: AppHandle) {
    let demo = std::env::var("DEMO_MODE").is_ok_and(|value| value == "1");
    // Every minute, over the last 5 minutes (a rolling window): updates feel
    // quick, and one stray tab doesn't flip the topic. The first runs soon after launch.
    let (first_run, every, window_minutes) = if demo {
        (Duration::from_secs(30), Duration::from_secs(30), 1)
    } else {
        (Duration::from_secs(15), Duration::from_secs(60), 5)
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
                run_once(&buffer, &sharing, &llm, &dev, &apps, window_minutes).await
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
    let apps = recent_apps(crate::db::get_db_path(&app).as_deref(), WINDOW_MINUTES);
    let summary = run_once(&buffer, &sharing, &llm, &dev, &apps, WINDOW_MINUTES)
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
    window_minutes: u32,
) -> Option<ActivitySummary> {
    if !sharing.is_on() {
        return None;
    }
    let epoch = sharing.epoch();
    // Pages stay in the buffer until the sweeper drops them (6 min), so each
    // run reads only the ones from its window.
    let since = buffer::now_ms() - i64::from(window_minutes) * 60_000;
    let captures: Vec<Capture> = match buffer.read_all() {
        Ok(captures) => captures.into_iter().filter(|capture| capture.captured_at >= since).collect(),
        Err(err) => {
            eprintln!("[summarizer] reading the buffer failed: {err}");
            return None;
        }
    };
    if !captures.is_empty() || !apps.is_empty() {
        // If it isn't ready in time, chat() fails fast and the summary falls back.
        llm.wait_ready(READY_WAIT).await;
    }
    // Record what the model read and replied, for the Dev tab.
    let seen = Arc::new(std::sync::Mutex::new((None::<String>, Vec::<String>::new())));
    let started_at = buffer::now_ms();
    let summary = summarize(&captures, apps, started_at, window_minutes, |prompt| {
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

    if !sharing.is_on() || sharing.epoch() != epoch {
        return None;
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
    now: i64,
    window_minutes: u32,
    mut ask_model: F,
) -> ActivitySummary
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let captures: Vec<Capture> = captures.iter().filter(|capture| !is_private_host(&capture.host)).cloned().collect();
    let apps: Vec<AppSession> = apps.iter().filter(|app| !is_private_app(app)).cloned().collect();
    if captures.is_empty() && apps.is_empty() {
        return ActivitySummary::not_shareable(Source::Idle, now, window_minutes);
    }
    let prompt = build_prompt(&captures, &apps);
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
                    Some((topic, activity, focus, task_summary)) => {
                        let reconciled = reconcile_topic(&topic, &activity);
                        let ruled = leetcode_topic(&topic, &captures, &apps)
                            .or_else(|| leetcode_topic(&reconciled, &captures, &apps))
                            .or_else(|| job_topic(&reconciled, &captures, &apps))
                            .or_else(|| course_topic(&reconciled, &captures, &apps));
                        let task_summary = ruled
                            .as_deref()
                            .filter(|ruled| *ruled != reconciled)
                            .and_then(|ruled| summary_after_screen_rule(ruled, &focus, &task_summary))
                            .unwrap_or(task_summary);
                        ActivitySummary {
                            shareable: true,
                            focus,
                            topic: ruled.unwrap_or(reconciled),
                            task_summary,
                            source: Source::Llama,
                            window_minutes,
                            created_at: now,
                        }
                    }
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
// Each summary stands alone: no previous topic or summary goes in. A
// "Previous topic" hint made the small model keep stale labels (Zillow
// browsing stayed "Coding", a biology quiz stayed "Web development").
fn build_prompt(captures: &[Capture], apps: &[AppSession]) -> String {
    let mut prompt = String::new();
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
        // Said next to the page, where a small model reads it: the posting's duties
        // aren't the person's task.
        let note = if is_job_board(capture) { " (job listings they are browsing, not their own work)" } else { "" };
        let entry = format!(
            "\n[{}] {}{} | {}{}\n{}\n",
            clock_time(capture.captured_at),
            capture.host,
            capture.path.trim_end_matches('/'),
            capture.title,
            note,
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
/// Tool and app words that make a focus useless for matching ("yarn", "terminal").
/// A focus containing one is dropped (the summary is still shared).
const TOOL_WORDS: [&str; 16] = [
    "yarn", "npm", "pnpm", "pip", "git", "github", "terminal", "zsh", "vs code", "vscode",
    "rustrover", "intellij", "xcode", "docker", "chrome", "jupyter",
];

fn names_a_tool(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    TOOL_WORDS.iter().any(|tool| if tool.contains(' ') { lower.contains(tool) } else { words.contains(tool) })
}

/// The topic is free text, but one that names a tool ("Yarn", "Git rebase") says what's
/// on screen, not what the person is doing. Those tools are all for writing software.
fn clean_topic(topic: &str) -> String {
    if names_a_tool(topic) { "Coding".to_string() } else { topic.to_string() }
}

/// The model names the activity in its own words before picking a label, and its own
/// words are often right when the label isn't ("Knitting" → Writing, "Video editing" →
/// Web development): with a list in front of it, a small model reaches for the nearest
/// label. Keep the label when the two agree (same label, or a shared word: "Ui design" /
/// Design, "Job searching" / Job hunting); otherwise trust the model's own words.
/// Other names the model uses for a list label, so its own words map back onto the list.
const TOPIC_ALIASES: [(&str, &str); 24] = [
    ("programming", "Coding"),
    ("software development", "Coding"),
    ("software engineering", "Coding"),
    ("development", "Coding"),
    ("computer science", "Coding"),
    ("web design", "Web development"),
    ("frontend", "Web development"),
    ("backend", "Web development"),
    ("mobile development", "App development"),
    ("ios development", "App development"),
    ("data analysis", "Data science"),
    ("analytics", "Data science"),
    ("ai", "Machine learning"),
    ("real estate", "Apartment hunting"),
    ("housing", "Apartment hunting"),
    ("apartment search", "Apartment hunting"),
    ("house hunting", "Apartment hunting"),
    ("home search", "Apartment hunting"),
    ("job search", "Job hunting"),
    ("careers", "Job hunting"),
    ("career", "Job hunting"),
    ("study", "Studying"),
    ("learning", "Studying"),
    ("education", "Studying"),
];

fn reconcile_topic(label: &str, activity: &str) -> String {
    let own = clean_topic(&first_words(&scrub(activity), MAX_TOPIC_WORDS, MAX_TOPIC_CHARS));
    if own.is_empty() || own.eq_ignore_ascii_case(OTHER_TOPIC) {
        return label.to_string();
    }
    if let Some(known) = KNOWN_TOPICS.iter().find(|known| known.eq_ignore_ascii_case(&own)) {
        return known.to_string();
    }
    // Before the same-as-label check: after Other, the label is these same words.
    if let Some((_, known)) = TOPIC_ALIASES.iter().find(|(alias, _)| alias.eq_ignore_ascii_case(&own)) {
        return known.to_string();
    }
    if own.eq_ignore_ascii_case(label) {
        return label.to_string();
    }
    // Generic action words say nothing about the subject: "Flight hunting" shares
    // "hunting" with Job hunting but isn't job hunting. Only subject words count.
    const GENERIC: [&str; 14] = [
        "and", "for", "the", "with", "hunting", "hunt", "search", "searching", "looking",
        "finding", "browsing", "booking", "planning", "research",
    ];
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.len() > 2 && !GENERIC.contains(word))
            .map(str::to_string)
            .collect()
    };
    // Same word, or the same stem ("study" / "studying", "design" / "designing").
    let stem = |word: &str| word.chars().take(5).collect::<String>();
    let label_stems: Vec<String> = words(label).iter().map(|word| stem(word)).collect();
    if words(&own).iter().any(|word| label_stems.contains(&stem(word))) {
        return label.to_string();
    }
    let mut chars = own.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// "main.js", "App.jsx", "hw3.ipynb": a file name says what's open, not what they're doing.
static FILE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w\.[A-Za-z][A-Za-z0-9]{0,4}\b").unwrap());

fn clean_focus(focus: &str, topic: &str) -> String {
    let focus = first_words(&scrub(focus), 4, MAX_TOPIC_CHARS);
    let names_a_file = FILE_NAME.is_match(&focus);
    if focus.is_empty() || names_a_tool(&focus) || names_a_file || looks_identifying(&focus) || focus.eq_ignore_ascii_case(topic) {
        return String::new();
    }
    focus
}

fn clean(output: ModelOutput) -> Option<(String, String, String, String)> {
    if !output.shareable {
        return None;
    }
    // "Other": use the model's own name for the activity; none usable means not shared.
    let output = if output.topic.eq_ignore_ascii_case(OTHER_TOPIC) {
        let own = first_words(&scrub(&output.activity), MAX_TOPIC_WORDS, MAX_TOPIC_CHARS);
        if own.is_empty() || own.eq_ignore_ascii_case(OTHER_TOPIC) {
            return None;
        }
        ModelOutput { topic: own, ..output }
    } else {
        output
    };
    let topic = scrub(&output.topic);
    let task_summary = crate::scrub::scrub_uncut(&output.task_summary);
    if looks_identifying(&topic) || looks_identifying(&task_summary) {
        return None;
    }
    let task_summary = cut_at_word(&task_summary, MAX_SUMMARY_CHARS);
    let topic = canonical_topic(&clean_topic(&first_words(&topic, MAX_TOPIC_WORDS, MAX_TOPIC_CHARS)), &task_summary);
    let focus = clean_focus(&output.focus, &topic);
    let activity = output.activity.clone();
    (!topic.is_empty() && !task_summary.is_empty()).then_some((topic, activity, focus, task_summary))
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

#[tauri::command]
pub fn get_last_summary(buffer: State<'_, Buffer>) -> Result<Option<ActivitySummary>, String> {
    let Some(json) = buffer.last_summary().map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    serde_json::from_str(&json).map(Some).map_err(|err| err.to_string())
}

#[cfg(test)]
#[path = "topic_eval.rs"]
mod topic_eval;

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_761_300_300_000;

    fn capture(id: i64, host: &str, path: &str, title: &str, text: &str) -> Capture {
        Capture {
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
            activity: String::new(),
            focus: String::new(),
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
            Some(("LeetCode".into(), String::new(), String::new(), "Working through graph problems on LeetCode".into()))
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
        let long_summary = "Working through graph problems ".repeat(20);
        let (topic, _activity, _focus, task_summary) =
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
        let summary = summarize(&[], &[], NOW, 5, |_| {
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
        let summary = summarize(&leetcode(), &[], NOW, 5, |_| {
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
        let summary = summarize(&leetcode(), &[], NOW, 5, |_| {
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
        let summary = summarize(&leetcode(), &[], NOW, 5, |prompt| {
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
        let summary = summarize(&leetcode(), &[], NOW, 5, |_| async {
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
        let prompt = build_prompt(&captures, &[]);

        assert!(!prompt.contains("Previous topic"), "no carry-over between summaries");
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
        assert!(build_prompt(&many, &[]).chars().count() < MAX_PROMPT_CHARS + 100);
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
    fn private_app_names_match_whole_words() {
        assert!(is_private_app(&app("Microsoft Outlook", "", 0, 1)));
        assert!(is_private_app(&app("Slack", "", 0, 1)));
        assert!(!is_private_app(&app("Visual Studio Code", "dijkstra.py", 0, 1)));
        assert!(!is_private_app(&app("Mailspring Notes", "", 0, 1)));
    }

    #[test]
    fn apps_section_merges_repeats_and_puts_most_time_first() {
        let apps = [
            app("Chrome", "Two Sum - LeetCode", 0, 60_000),
            app("Code", "dijkstra.py", 60_000, 300_000),
            app("Chrome", "Two Sum - LeetCode", 300_000, 360_000),
        ];
        let prompt = build_prompt(&[], &apps);
        let code = prompt.find("Code: dijkstra.py (4 min)").expect("code line");
        let chrome = prompt.find("Chrome: Two Sum - LeetCode (2 min)").expect("merged chrome line");
        assert!(code < chrome);
        assert!(!prompt.contains("Pages from"));
    }

    #[test]
    fn course_codes_come_from_titles_on_screen() {
        let apps = [app("Google Chrome", "Project 2: Gitlet | CS 61B Spring 2027", 0, 60_000)];
        assert_eq!(course_topic("Coding", &[], &apps).as_deref(), Some("CS 61B"));
        let pdf = [app("Preview", "Physics 7B Lab 3 - Overleaf", 0, 60_000)];
        assert_eq!(course_topic("Writing", &[], &pdf).as_deref(), Some("Physics 7B"));
        let notes = [app("Notion", "Bio 101 – Cell respiration notes", 0, 60_000)];
        assert_eq!(course_topic("Studying", &[], &notes).as_deref(), Some("Bio 101"));
        // Not courses, or not course work.
        let not_course = [app("Google Chrome", "Note 12: Probability, Summer 2027", 0, 60_000)];
        assert_eq!(course_topic("Studying", &[], &not_course), None);
        assert_eq!(course_topic("Job hunting", &[], &apps), None);
    }

    #[test]
    fn leetcode_on_screen_wins_over_problem_names() {
        let captures = [capture(1, "leetcode.com", "/problems/course-schedule/", "Course Schedule - LeetCode", "")];
        assert_eq!(leetcode_topic("Coursework", &captures, &[]).as_deref(), Some("LeetCode"));
        assert_eq!(leetcode_topic("Job hunting", &captures, &[]), None);
        assert_eq!(leetcode_topic("Coding", &[], &[]), None);
        let neetcode = [capture(1, "neetcode.io", "/problems/meeting-schedule", "Meeting Schedule - NeetCode", "")];
        assert_eq!(leetcode_topic("Coding", &neetcode, &[]).as_deref(), Some("LeetCode"));
        let neetcode_tab = [app("Google Chrome", "Meeting Schedule - NeetCode", 0, 60_000)];
        assert_eq!(leetcode_topic("Web development", &[], &neetcode_tab).as_deref(), Some("LeetCode"));
    }

    #[test]
    fn generic_words_alone_dont_keep_a_label() {
        // Seen live: "Searching for flights to Vancouver" came out as Job hunting.
        assert_eq!(reconcile_topic("Job hunting", "Flight hunting"), "Flight hunting");
        assert_eq!(reconcile_topic("Job hunting", "House hunting"), "Apartment hunting");
        assert_eq!(reconcile_topic("Job hunting", "Job searching"), "Job hunting");
        assert_eq!(reconcile_topic("Design", "Ui design"), "Design");
    }

    #[test]
    fn job_boards_are_job_hunting_whatever_the_posting() {
        let posting = |id, host: &str, path: &str| capture(id, host, path, "Research Intern jobs", "Research Scientist Intern");
        let linkedin = [posting(1, "www.linkedin.com", "/jobs/search/"), posting(2, "boards.greenhouse.io", "/acme/jobs/1")];
        let chrome = [app("Google Chrome", "", 0, 240_000), app("Code", "main.rs", 0, 60_000)];
        assert_eq!(job_topic("Research", &linkedin, &chrome).as_deref(), Some("Job hunting"));
        assert_eq!(job_topic("Coding", &linkedin, &[]).as_deref(), Some("Job hunting"));
        // A cover letter next to the posting stays Writing.
        assert_eq!(job_topic("Writing", &linkedin, &chrome), None);
        // A glance at jobs while another app had most of the time.
        let coding = [app("Google Chrome", "", 0, 60_000), app("Code", "main.rs", 0, 240_000)];
        assert_eq!(job_topic("Coding", &linkedin, &coding), None);
        // Job pages aren't most of the pages.
        let mixed = [posting(1, "www.linkedin.com", "/jobs/view/4012345/"), posting(2, "react.dev", "/learn"), posting(3, "github.com", "/")];
        assert_eq!(job_topic("Coding", &mixed, &chrome), None);
        // LinkedIn outside /jobs is not a job board.
        assert!(!is_job_board(&posting(1, "www.linkedin.com", "/feed/")));
        assert!(is_job_board(&posting(1, "acme.myworkdayjobs.com", "/en-US/careers")));
    }

    #[tokio::test]
    async fn a_moved_topic_replaces_a_summary_of_the_page() {
        let model = |topic: &'static str, focus: &'static str, task_summary: &'static str| {
            move |_| async move {
                Ok(json!({ "shareable": true, "topic": topic, "activity": topic, "focus": focus, "task_summary": task_summary })
                    .to_string())
            }
        };
        let chrome = [app("Google Chrome", "", 0, 300_000)];
        let linkedin = [capture(1, "www.linkedin.com", "/jobs/search/", "(2) Research Intern jobs | LinkedIn", "Research Scientist Intern")];
        // The posting's duties, written as the person's task.
        let duties = summarize(&linkedin, &chrome, NOW, 5, model("Research", "Recommendation Systems", "Developing large-scale ranking models")).await;
        assert_eq!((duties.topic.as_str(), duties.task_summary.as_str()), ("Job hunting", "Browsing job listings for Recommendation Systems roles"));
        // Already about the search: kept.
        let search = summarize(&linkedin, &chrome, NOW, 5, model("Research", "Research internships", "Looking for research internships")).await;
        assert_eq!(search.task_summary, "Looking for research internships");

        let neetcode = [capture(1, "neetcode.io", "/problems/meeting-schedule", "Meeting Schedule - NeetCode", "Given an array of meeting time intervals")];
        let statement = summarize(&neetcode, &chrome, NOW, 5, model("Coding", "Meeting Schedule", "Determining if a person can attend all meetings")).await;
        assert_eq!((statement.topic.as_str(), statement.task_summary.as_str()), ("LeetCode", "Practicing LeetCode problems: Meeting Schedule"));
        // The model already said LeetCode, so no rule moved the topic: its summary stands.
        let solving = summarize(&neetcode, &chrome, NOW, 5, model("LeetCode", "Meeting Schedule", "Checking meeting conflicts")).await;
        assert_eq!(solving.task_summary, "Checking meeting conflicts");
    }

    #[test]
    fn job_listings_are_labelled_in_the_prompt() {
        let captures = [
            capture(1, "www.linkedin.com", "/jobs/search/", "(2) Research Intern jobs | LinkedIn", "Research Scientist Intern"),
            capture(2, "react.dev", "/learn", "Quick Start - React", "Components"),
        ];
        let prompt = build_prompt(&captures, &[]);
        assert!(prompt.contains("Research Intern jobs | LinkedIn (job listings they are browsing, not their own work)"));
        assert!(prompt.contains("Quick Start - React\n"));
    }

    #[test]
    fn focus_names_the_subject_never_a_tool() {
        let with_focus = |focus: &str| ModelOutput { focus: focus.into(), ..output(true, "Coding", "Building a desktop app") };
        assert_eq!(clean(with_focus("desktop matching app")).unwrap().2, "desktop matching app");
        assert_eq!(clean(with_focus("Dijkstra's algorithm in Python today")).unwrap().2, "Dijkstra's algorithm in Python");
        for tool in ["yarn", "yarn dev server", "Git rebase", "VS Code setup", "Coding", "jane@example.com"] {
            let (topic, _, focus, _) = clean(with_focus(tool)).unwrap();
            assert_eq!((topic.as_str(), focus.as_str()), ("Coding", ""), "{tool}");
        }
        for file in ["main.js", "App.jsx component", "hw3.ipynb"] {
            assert_eq!(clean(with_focus(file)).unwrap().2, "", "{file}");
        }
        assert_eq!(clean(with_focus("Node.js REST API")).unwrap().2, "", "a dotted tech name is dropped too, which is fine");
        // "gitlet" is a project, not git.
        assert_eq!(clean(with_focus("Gitlet version control")).unwrap().2, "Gitlet version control");
    }

    #[test]
    fn summaries_saved_before_focus_still_load() {
        let old = r#"{"shareable":true,"topic":"LeetCode","task_summary":"Two Sum","source":"llama","window_minutes":5,"created_at":1}"#;
        let summary: ActivitySummary = serde_json::from_str(old).unwrap();
        assert_eq!(summary.focus, "");
    }

    #[test]
    fn own_words_win_when_the_label_is_unrelated() {
        assert_eq!(reconcile_topic("Writing", "Knitting"), "Knitting");
        assert_eq!(reconcile_topic("Web development", "Video editing"), "Video editing");
        assert_eq!(reconcile_topic("Research", "Git"), "Coding"); // a tool means coding
        assert_eq!(reconcile_topic("Design", "Ui design"), "Design");
        assert_eq!(reconcile_topic("Job hunting", "Job searching"), "Job hunting");
        assert_eq!(reconcile_topic("Web development", "coding"), "Coding");
        assert_eq!(reconcile_topic("Coding", ""), "Coding");
        assert_eq!(reconcile_topic("Studying", "gardening"), "Gardening");
        // Synonyms and stems map back onto the label.
        assert_eq!(reconcile_topic("Apartment hunting", "Real Estate"), "Apartment hunting");
        assert_eq!(reconcile_topic("Job hunting", "Real Estate"), "Apartment hunting");
        assert_eq!(reconcile_topic("Studying", "Study"), "Studying");
        assert_eq!(reconcile_topic("Web development", "Programming"), "Coding");
    }

    #[test]
    fn other_uses_the_models_own_label() {
        let other = |label: &str| ModelOutput { activity: label.into(), ..output(true, "Other", "Restoring an oak table") };
        assert_eq!(clean(other("Woodworking")).unwrap().0, "Woodworking");
        assert_eq!(clean(other("Yarn")).unwrap().0, "Coding");
        assert_eq!(clean(other("")), None);
        assert_eq!(clean(other("Other")), None);
    }

    #[test]
    fn topics_are_free_text_but_never_a_tool() {
        assert_eq!(clean(output(true, "Music production", "Mixing a hip hop beat")).unwrap().0, "Music production");
        assert_eq!(clean(output(true, "Yarn", "Running the desktop app")).unwrap().0, "Coding");
        assert_eq!(clean(output(true, "Git rebase", "Resolving merge conflicts")).unwrap().0, "Coding");
        assert_eq!(clean(output(true, "coding", "Fixing a bug")).unwrap().0, "Coding");
    }

    #[tokio::test]
    async fn private_pages_and_apps_never_reach_the_model() {
        let captures = vec![
            capture(1, "mail.google.com", "/mail/u/0", "Inbox - Gmail", "Interview invite"),
            capture(2, "secure.chase.com", "/dashboard", "Accounts", "balance"),
            capture(3, "mychart.example.org", "/results", "Test Results", "Ferritin LOW"),
            capture(4, "leetcode.com", "/problems/two-sum/", "Two Sum - LeetCode", "Given an array"),
        ];
        let apps = [app("Messages", "Mom", 0, 60_000), app("Google Chrome", "Inbox (3) - Gmail", 0, 60_000)];
        let summary = summarize(&captures, &apps, NOW, 5, |prompt| async move {
            for private in ["mail.google", "chase", "mychart", "Messages", "Gmail", "Ferritin"] {
                assert!(!prompt.contains(private), "{private} reached the model");
            }
            assert!(prompt.contains("leetcode.com"));
            Ok(reply("LeetCode", "Solving Two Sum on LeetCode"))
        })
        .await;
        assert!(summary.shareable);

        let only_private = summarize(&captures[..3], &apps, NOW, 5, |_| async {
            Err::<String, _>("the model must not be called".to_string())
        })
        .await;
        assert_eq!(only_private, ActivitySummary::not_shareable(Source::Idle, NOW, 5));
    }

    #[tokio::test]
    async fn our_own_video_calls_never_reach_the_model() {
        let captures = vec![
            capture(1, "meet.jit.si", "/workmatch-21-k3x9", "workmatch-21-k3x9 | Jitsi Meet", "Join meeting"),
            capture(2, "meet-jit-si-66cbd.firebaseapp.com", "/__/auth/handler", "Sign in", "Continue with Google"),
            capture(3, "www.zillow.com", "/san-francisco-ca/rentals/", "San Francisco CA Rentals", "1 bd apartment for rent"),
        ];
        let apps = [app("Google Chrome", "workmatch-21-k3x9 | Jitsi Meet", 0, 60_000)];
        let summary = summarize(&captures, &apps, NOW, 5, |prompt| async move {
            for call in ["jit.si", "jit-si", "Jitsi", "workmatch-21"] {
                assert!(!prompt.contains(call), "{call} reached the model");
            }
            assert!(prompt.contains("www.zillow.com"));
            Ok(reply("Apartment hunting", "Looking for a one-bedroom rental in San Francisco"))
        })
        .await;
        assert!(summary.shareable);
    }

    #[tokio::test]
    async fn other_with_a_known_synonym_lands_on_the_list() {
        let zillow = [capture(1, "www.zillow.com", "/san-francisco-ca/rentals/", "San Francisco CA Rentals", "1 bd apartment for rent")];
        let summary = summarize(&zillow, &[], NOW, 5, |_| async {
            Ok(json!({
                "shareable": true,
                "topic": "Other",
                "activity": "Real Estate",
                "task_summary": "Looking for a one-bedroom rental in San Francisco",
            })
            .to_string())
        })
        .await;
        assert_eq!(summary.topic, "Apartment hunting");
    }

    #[tokio::test]
    async fn app_sessions_alone_are_summarized_not_idle() {
        let apps = [app("Code", "dijkstra.py", 0, 300_000)];
        let summary = summarize(&[], &apps, NOW, 5, |_| async {
            Ok(reply("Coursework", "Implementing Dijkstra's algorithm"))
        })
        .await;
        assert_eq!(summary.source, Source::Llama);
        assert!(summary.shareable);
    }

}
