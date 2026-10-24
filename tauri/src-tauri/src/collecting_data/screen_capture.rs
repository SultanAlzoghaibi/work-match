// Reads the screen for the summarizer when the work isn't in Chrome: VS Code,
// PDFs, Figma, a terminal. Every CAPTURE_EVERY while sharing is on, it runs
// the wm-ocr sidecar (tauri/ocr-helper) on the front window and stores the
// text in the activity buffer as source "screen", next to the extension's
// Chrome pages, so the 5-minute summary (summarizer.rs) reads both.
//
// wm-ocr keeps the screenshot in memory and never writes it to disk, refuses
// windows titled like .env / credentials / id_rsa, replaces passwords, keys
// and tokens with [redacted], and drops browser toolbars. On top of that, this
// skips private apps, this app, and Chrome while the extension is sending
// pages (it reads the page itself, which beats reading pixels).
use crate::buffer::{self, Buffer, NewCapture};
use crate::scrub::scrub;
use crate::sharing::Sharing;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::ShellExt;
use tokio::time::MissedTickBehavior;

const CAPTURE_EVERY: Duration = Duration::from_secs(30);
const DEMO_CAPTURE_EVERY: Duration = Duration::from_secs(10);
/// Chrome is left to the extension while it has sent a page this recently.
const EXTENSION_FRESH_MS: i64 = 2 * 60 * 1000;
const MAX_TEXT_CHARS: usize = 2000;
const CHROME: &str = "com.google.Chrome";

/// Never captured: messaging, mail, calls, passwords, system settings, and this
/// app. wm-ocr checks the bundle before taking any screenshot.
const SKIP_BUNDLES: [&str; 18] = [
    "com.apple.MobileSMS",
    "com.apple.mail",
    "com.apple.FaceTime",
    "com.apple.Passwords",
    "com.apple.keychainaccess",
    "com.apple.systempreferences",
    "com.apple.loginwindow",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.tinyspeck.slackmacgap",
    "com.hnc.Discord",
    "net.whatsapp.WhatsApp",
    "ru.keepcoder.Telegram",
    "org.whispersystems.signal-desktop",
    "com.microsoft.Outlook",
    "com.sultanalzoghaibi.workmatch",
    "com.sultanalzoghaibi.workmatch2",
];

/// In `tauri dev` the app runs unbundled, so it has no bundle id to skip by.
const OWN_APP_NAMES: [&str; 2] = ["work_match", "work-match"];

/// The part of wm-ocr's JSON this uses (see tauri/ocr-helper/README.md).
#[derive(Debug, Default, Deserialize)]
struct OcrResult {
    app: Option<String>,
    window_title: Option<String>,
    #[serde(default)]
    main_text: String,
    #[serde(default)]
    skipped: bool,
    error: Option<String>,
}

/// One buffer row's worth of text: (app name, window title, text).
#[derive(Debug, PartialEq)]
struct ScreenText {
    app: String,
    title: String,
    text: String,
}

pub fn start(app: AppHandle) {
    if !sidecar_is_real() {
        eprintln!("[screen] wm-ocr sidecar is missing or the build placeholder (run tauri/ocr-helper/build.sh); not reading the screen");
        return;
    }
    let demo = std::env::var("DEMO_MODE").is_ok_and(|value| value == "1");
    let every = if demo { DEMO_CAPTURE_EVERY } else { CAPTURE_EVERY };
    let buffer = app.state::<Buffer>().inner().clone();
    let sharing = app.state::<Sharing>().inner().clone();

    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(every);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut asked_for_permission = false;
        loop {
            ticker.tick().await;
            if !sharing.is_on() {
                continue;
            }
            let extension_active = buffer.has_recent("chrome", buffer::now_ms() - EXTENSION_FRESH_MS);
            let mut args = vec!["--no-correction".to_string(), "--skip-bundle".to_string(), skip_list(extension_active)];
            if !asked_for_permission {
                // One-time macOS prompt for Screen Recording.
                asked_for_permission = true;
                args.push("--request-permission".into());
            }
            let output = match app.shell().sidecar("wm-ocr") {
                Ok(command) => command.args(args).output().await,
                Err(err) => {
                    eprintln!("[screen] could not start wm-ocr: {err}");
                    continue;
                }
            };
            let output = match output {
                Ok(output) => output,
                Err(err) => {
                    eprintln!("[screen] wm-ocr failed: {err}");
                    continue;
                }
            };
            let result: OcrResult = serde_json::from_slice(&output.stdout).unwrap_or_default();
            if let Some(error) = &result.error {
                // no_window is normal (desktop in front); only the rest is worth a line.
                if error == "screen_recording_permission" {
                    eprintln!("[screen] no Screen Recording permission: grant it in System Settings > Privacy & Security, then relaunch");
                } else if error != "no_window" {
                    eprintln!("[screen] wm-ocr error: {error}");
                }
                continue;
            }
            let Some(screen) = screen_text(&result) else {
                continue;
            };
            let text_hash = format!("{:x}", Sha256::digest(screen.text.as_bytes()));
            let row = NewCapture {
                captured_at: buffer::now_ms(),
                source: "screen",
                host: &screen.app,
                // Each window is its own "page", so two VS Code files don't replace each other.
                path: &screen.title,
                title: &screen.title,
                text: &screen.text,
                text_hash: &text_hash,
            };
            match buffer.insert(&row) {
                Ok(true) if crate::collecting_data::debug_logs() => {
                    println!("[screen] {} {} chars", screen.app, screen.text.chars().count())
                }
                Ok(_) => {}
                Err(err) => eprintln!("[screen] insert failed: {err}"),
            }
        }
    });
}

/// The apps wm-ocr must skip this time. Chrome is only added while the
/// extension is covering it.
fn skip_list(extension_active: bool) -> String {
    let mut bundles: Vec<&str> = SKIP_BUNDLES.to_vec();
    if extension_active {
        bundles.push(CHROME);
    }
    bundles.join(",")
}

/// What to store from one wm-ocr run, or None when there's nothing to keep.
/// Text is scrubbed (emails, phones, long numbers) on top of wm-ocr's own
/// secret redaction, then cut to MAX_TEXT_CHARS.
fn screen_text(result: &OcrResult) -> Option<ScreenText> {
    if result.skipped {
        return None;
    }
    let app = result.app.as_deref()?.trim();
    if app.is_empty() || OWN_APP_NAMES.iter().any(|own| app.eq_ignore_ascii_case(own)) {
        return None;
    }
    let text: String = scrub(&result.main_text).chars().take(MAX_TEXT_CHARS).collect();
    if text.trim().is_empty() {
        return None;
    }
    let title = scrub(result.window_title.as_deref().unwrap_or(""));
    Some(ScreenText { app: app.to_string(), title, text })
}

/// build.rs stands in an empty file when binaries/wm-ocr-<triple> is missing,
/// so the app still builds without the sidecar.
fn sidecar_is_real() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(format!("wm-ocr{}", std::env::consts::EXE_SUFFIX))))
        .and_then(|path| std::fs::metadata(path).ok())
        .is_some_and(|meta| meta.len() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> OcrResult {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn keeps_app_title_and_scrubbed_text() {
        let result = parse(
            r#"{"app":"Code","bundle_id":"com.microsoft.VSCode","window_title":"dijkstra.py — cs170",
                "main_text":"def dijkstra(graph, src):\nemail me at a@b.com","skipped":false,"redactions":0}"#,
        );
        let screen = screen_text(&result).unwrap();
        assert_eq!(screen.app, "Code");
        assert_eq!(screen.title, "dijkstra.py — cs170");
        assert!(screen.text.contains("def dijkstra(graph, src):"));
        assert!(screen.text.contains("[email]") && !screen.text.contains("a@b.com"));
    }

    #[test]
    fn skips_skipped_empty_and_own_windows() {
        assert_eq!(screen_text(&parse(r#"{"app":"Messages","skipped":true,"skip_reason":"blocklist"}"#)), None);
        assert_eq!(screen_text(&parse(r#"{"app":"Code","main_text":"   "}"#)), None);
        assert_eq!(screen_text(&parse(r#"{"app":"work_match","main_text":"Your summary"}"#)), None);
        assert_eq!(screen_text(&parse(r#"{"main_text":"no app name"}"#)), None);
    }

    #[test]
    fn errors_parse_and_store_nothing() {
        let result = parse(r#"{"error":"screen_recording_permission","message":"Grant Screen Recording"}"#);
        assert_eq!(result.error.as_deref(), Some("screen_recording_permission"));
        assert_eq!(screen_text(&result), None);
    }

    #[test]
    fn cuts_long_text() {
        let long = "word ".repeat(1000);
        let result = OcrResult { app: Some("Preview".into()), main_text: long, ..Default::default() };
        assert_eq!(screen_text(&result).unwrap().text.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn chrome_is_left_to_the_extension_only_while_it_is_active() {
        assert!(skip_list(true).split(',').any(|bundle| bundle == CHROME));
        assert!(!skip_list(false).split(',').any(|bundle| bundle == CHROME));
        for private in ["com.apple.MobileSMS", "com.1password.1password", "com.sultanalzoghaibi.workmatch"] {
            assert!(skip_list(false).split(',').any(|bundle| bundle == private), "{private}");
        }
    }

    #[test]
    fn recent_chrome_capture_is_detected() {
        let buffer = Buffer::open_in_memory();
        let now = buffer::now_ms();
        assert!(!buffer.has_recent("chrome", now - EXTENSION_FRESH_MS));
        buffer
            .insert(&NewCapture {
                captured_at: now,
                source: "chrome",
                host: "leetcode.com",
                path: "/problems/two-sum/",
                title: "Two Sum",
                text: "Given an array",
                text_hash: "h",
            })
            .unwrap();
        assert!(buffer.has_recent("chrome", now - EXTENSION_FRESH_MS));
        assert!(!buffer.has_recent("screen", now - EXTENSION_FRESH_MS));
    }
}
