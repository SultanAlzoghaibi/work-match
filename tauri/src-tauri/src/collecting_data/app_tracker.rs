// Records which app is in front, its focused window's title and the document
// it has open, as timestamped rows in work_match.db (app_sessions). Runs every
// POLL_EVERY while tracking is on (sharing.rs, which follows friend finding)
// and keeps a history; the 5-minute summary reads the recent rows (summarizer.rs).
//
// macOS APIs:
// - front app: NSWorkspace.frontmostApplication (no permission needed)
// - window title + document: Accessibility (AXFocusedWindow -> AXTitle /
//   AXDocument). Needs the Accessibility permission; without it only the app
//   name and bundle id are recorded.
//
// Windows APIs (no permission needed):
// - front app: GetForegroundWindow -> process exe -> its FileDescription
//   ("Visual Studio Code"); the exe name ("code.exe") stands in for the bundle id
// - window title: GetWindowTextW, except for browsers (see BROWSER_EXES)
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const POLL_EVERY: Duration = Duration::from_secs(2);

/// Apps whose window titles are private (messages, mail, passwords, settings):
/// only the app name is recorded. macOS bundle ids, then Windows exe names.
const PRIVATE_APPS: &[&str] = &[
    "com.apple.MobileSMS",
    "com.apple.mail",
    "com.apple.FaceTime",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.apple.keychainaccess",
    "com.apple.Passwords",
    "com.apple.systempreferences",
    "com.apple.loginwindow",
    "olk.exe",
    "outlook.exe",
    "hxoutlook.exe",
    "whatsapp.exe",
    "1password.exe",
    "bitwarden.exe",
    "keepassxc.exe",
    "systemsettings.exe",
    "lockapp.exe",
    "credentialuibroker.exe",
];

/// Windows browsers: their window title is the current tab's title, incognito
/// tabs included, so it's never recorded. The Chrome extension covers pages.
#[cfg(any(windows, test))]
const BROWSER_EXES: [&str; 7] = [
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "arc.exe",
];

/// Our own app (both dev instances) isn't recorded at all.
#[cfg(target_os = "macos")]
const OWN_BUNDLE_PREFIX: &str = "com.sultanalzoghaibi.workmatch";
/// The dev build's process name (no bundle id outside a packaged .app).
#[cfg(target_os = "macos")]
const OWN_DEV_BINARY: &str = "work_match";

/// What's in front right now. Two polls with an equal Focus are the same session.
#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub document: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AppSession {
    pub id: i64,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
    pub document: Option<String>,
    pub started_at: i64,
    pub ended_at: i64,
}

impl Focus {
    /// Drop details for private apps and scrub what's kept (emails, phone
    /// numbers, long numbers) before anything is stored.
    fn cleaned(mut self) -> Self {
        let private = self
            .bundle_id
            .as_deref()
            .is_some_and(|bundle| PRIVATE_APPS.contains(&bundle));
        if private {
            self.window_title = None;
            self.document = None;
        }
        let clean = |text: Option<String>| {
            text.map(|text| crate::scrub::scrub(text.trim()))
                .filter(|text| !text.is_empty())
        };
        self.window_title = clean(self.window_title);
        self.document = clean(self.document);
        self
    }
}

/// Session bookkeeping over an open connection (in-memory in tests).
pub struct Recorder {
    conn: Connection,
    open: Option<(i64, Focus)>,
}

impl Recorder {
    pub fn new(conn: Connection) -> Self {
        Self { conn, open: None }
    }

    /// One poll: extend the open session if nothing changed, otherwise end it
    /// and start a new one. `None` (sharing off, our own app, nothing in
    /// front) just ends the open session.
    pub fn observe(&mut self, focus: Option<Focus>, now: i64) -> rusqlite::Result<()> {
        let focus = focus.map(Focus::cleaned);
        if let (Some((id, open)), Some(focus)) = (&self.open, &focus) {
            if open == focus {
                self.conn
                    .execute("UPDATE app_sessions SET ended_at = ?1 WHERE id = ?2", params![now, id])?;
                return Ok(());
            }
        }
        self.open = None;
        if let Some(focus) = focus {
            self.conn.execute(
                "INSERT INTO app_sessions (app_name, bundle_id, window_title, document, started_at, ended_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![focus.app_name, focus.bundle_id, focus.window_title, focus.document, now],
            )?;
            if super::debug_logs() {
                println!("[app-tracker] now in front: {}", describe(&focus));
            }
            self.open = Some((self.conn.last_insert_rowid(), focus));
        }
        Ok(())
    }
}

/// "Code: dijkstra.py — cs170-hw4 [cs170-hw4/dijkstra.py]" for the debug log.
fn describe(focus: &Focus) -> String {
    let mut text = focus.app_name.clone();
    if let Some(title) = &focus.window_title {
        text.push_str(&format!(": {title}"));
    }
    if let Some(document) = &focus.document {
        text.push_str(&format!(" [{document}]"));
    }
    if focus.window_title.is_none() && focus.document.is_none() {
        text.push_str(" (no title: private app or no Accessibility permission)");
    }
    text
}

/// Sessions seen in front at or after `since_ms`, oldest first.
pub fn sessions_since(conn: &Connection, since_ms: i64) -> rusqlite::Result<Vec<AppSession>> {
    let mut statement = conn.prepare(
        "SELECT id, app_name, bundle_id, window_title, document, started_at, ended_at
         FROM app_sessions WHERE ended_at >= ?1 ORDER BY started_at",
    )?;
    let rows = statement.query_map([since_ms], |row| {
        Ok(AppSession {
            id: row.get(0)?,
            app_name: row.get(1)?,
            bundle_id: row.get(2)?,
            window_title: row.get(3)?,
            document: row.get(4)?,
            started_at: row.get(5)?,
            ended_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// For the summarizer: what was in front during the last window.
pub fn recent_sessions(db_path: &Path, since_ms: i64) -> Vec<AppSession> {
    Connection::open(db_path)
        .and_then(|conn| sessions_since(&conn, since_ms))
        .unwrap_or_default()
}

/// Debug / future UI: the latest sessions, newest first.
#[tauri::command]
pub fn get_app_sessions(app: AppHandle, limit: Option<u32>) -> Result<Vec<AppSession>, String> {
    let path = crate::db::get_db_path(&app).ok_or("no database path")?;
    let conn = Connection::open(path).map_err(|err| err.to_string())?;
    let mut sessions = sessions_since(&conn, 0).map_err(|err| err.to_string())?;
    sessions.reverse();
    sessions.truncate(limit.unwrap_or(50) as usize);
    Ok(sessions)
}

/// For the Dev tab: can we read window titles? Never prompts.
pub fn accessibility_granted() -> bool {
    platform::accessibility_trusted(false)
}

pub fn start(app: AppHandle) {
    let Some(db_path) = crate::db::get_db_path(&app) else {
        eprintln!("[app-tracker] no database path; not tracking apps");
        return;
    };
    std::thread::spawn(move || {
        let mut recorder = Recorder::new(wait_for_table(&db_path));
        let mut asked_for_permission = false;
        loop {
            let sharing_on = app.state::<crate::sharing::Sharing>().is_on();
            let focus = if sharing_on {
                if !asked_for_permission {
                    // One-time macOS prompt; titles stay empty until it's granted.
                    asked_for_permission = true;
                    if !platform::accessibility_trusted(true) {
                        println!("[app-tracker] no Accessibility permission yet: recording app names only");
                    }
                }
                platform::current_focus(&app)
            } else {
                None
            };
            if let Err(err) = recorder.observe(focus, crate::buffer::now_ms()) {
                eprintln!("[app-tracker] writing app_sessions failed: {err}");
            }
            std::thread::sleep(POLL_EVERY);
        }
    });
}

/// The sql plugin runs migration 005 when the app starts; wait for it.
fn wait_for_table(db_path: &PathBuf) -> Connection {
    loop {
        if let Ok(conn) = Connection::open(db_path) {
            let exists = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'app_sessions'",
                    [],
                    |_| Ok(()),
                )
                .optional()
                .ok()
                .flatten()
                .is_some();
            if exists {
                return conn;
            }
        }
        std::thread::sleep(POLL_EVERY);
    }
}

/// Keep only the parent folder and file name of an AXDocument file URL.
#[cfg(target_os = "macos")]
fn short_document(url: &str) -> Option<String> {
    let path = url::Url::parse(url).ok()?.to_file_path().ok()?;
    let file = path.file_name()?.to_string_lossy().into_owned();
    match path.parent().and_then(Path::file_name) {
        Some(folder) => Some(format!("{}/{file}", folder.to_string_lossy())),
        None => Some(file),
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{short_document, Focus, OWN_BUNDLE_PREFIX, OWN_DEV_BINARY};
    use accessibility_sys::{
        kAXDocumentAttribute, kAXErrorSuccess, kAXFocusedWindowAttribute, kAXTitleAttribute,
        kAXTrustedCheckOptionPrompt, AXIsProcessTrustedWithOptions, AXUIElementCopyAttributeValue,
        AXUIElementCreateApplication, AXUIElementRef, AXUIElementSetMessagingTimeout,
    };
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use std::sync::mpsc;
    use std::time::Duration;
    use tauri::AppHandle;

    /// Don't let a frozen app block the tracker.
    const AX_TIMEOUT_SECS: f32 = 0.5;

    pub fn accessibility_trusted(prompt: bool) -> bool {
        unsafe {
            let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
            let value = if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() };
            let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
            AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
        }
    }

    pub fn current_focus(app: &AppHandle) -> Option<Focus> {
        let (app_name, bundle_id, pid) = frontmost_app(app)?;
        // Dev builds run the bare `work_match` binary, which has no bundle id.
        let own_app = bundle_id.as_deref().is_some_and(|bundle| bundle.starts_with(OWN_BUNDLE_PREFIX))
            || app_name == OWN_DEV_BINARY;
        if own_app {
            return None;
        }
        let (window_title, document) = focused_window(pid);
        Some(Focus { app_name, bundle_id, window_title, document })
    }

    /// NSWorkspace is read on the main thread, where AppKit keeps it current.
    fn frontmost_app(app: &AppHandle) -> Option<(String, Option<String>, i32)> {
        let (sender, receiver) = mpsc::channel();
        app.run_on_main_thread(move || {
            use tauri_nspanel::objc2_app_kit::NSWorkspace;
            let front = NSWorkspace::sharedWorkspace().frontmostApplication();
            let _ = sender.send(front.map(|running| {
                (
                    running.localizedName().map(|name| name.to_string()).unwrap_or_default(),
                    running.bundleIdentifier().map(|bundle| bundle.to_string()),
                    running.processIdentifier(),
                )
            }));
        })
        .ok()?;
        let (name, bundle_id, pid) = receiver.recv_timeout(Duration::from_secs(1)).ok()??;
        let name = if name.is_empty() { bundle_id.clone().unwrap_or_default() } else { name };
        (!name.is_empty()).then_some((name, bundle_id, pid))
    }

    /// (title, document) of the app's focused window; None without permission.
    fn focused_window(pid: i32) -> (Option<String>, Option<String>) {
        unsafe {
            let app_element = AXUIElementCreateApplication(pid);
            if app_element.is_null() {
                return (None, None);
            }
            // Releases the element when dropped.
            let _app_owner = CFType::wrap_under_create_rule(app_element as CFTypeRef);
            AXUIElementSetMessagingTimeout(app_element, AX_TIMEOUT_SECS);
            let Some(window) = copy_attribute(app_element, kAXFocusedWindowAttribute) else {
                return (None, None);
            };
            let window_element = window.as_CFTypeRef() as AXUIElementRef;
            let title = string_attribute(window_element, kAXTitleAttribute);
            let document = string_attribute(window_element, kAXDocumentAttribute)
                .and_then(|url| short_document(&url));
            (title, document)
        }
    }

    unsafe fn copy_attribute(element: AXUIElementRef, attribute: &str) -> Option<CFType> {
        let name = CFString::new(attribute);
        let mut value: CFTypeRef = std::ptr::null();
        let error = AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value);
        (error == kAXErrorSuccess && !value.is_null()).then(|| CFType::wrap_under_create_rule(value))
    }

    unsafe fn string_attribute(element: AXUIElementRef, attribute: &str) -> Option<String> {
        copy_attribute(element, attribute)?
            .downcast::<CFString>()
            .map(|text| text.to_string())
    }
}

/// Windows: the Focus for the foreground process's exe path, its
/// FileDescription and the window title. None for our own app (any instance).
#[cfg(any(windows, test))]
fn windows_focus(exe_path: &str, description: Option<String>, title: Option<String>) -> Option<Focus> {
    let file = exe_path.rsplit(['\\', '/']).next().unwrap_or(exe_path);
    let exe = file.to_lowercase();
    if exe.starts_with("work_match") || exe.starts_with("work-match") {
        return None;
    }
    let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    let app_name = description
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| stem.to_string());
    let window_title = title.filter(|_| !BROWSER_EXES.contains(&exe.as_str()));
    Some(Focus { app_name, bundle_id: Some(exe), window_title, document: None })
}

#[cfg(windows)]
mod platform {
    use super::Focus;
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};
    use tauri::AppHandle;
    use windows::core::{BOOL, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    /// exe path -> FileDescription, so the version resource is read once per app.
    static DESCRIPTIONS: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(Default::default);

    /// Any app can read window titles on Windows; there's nothing to grant.
    pub fn accessibility_trusted(_prompt: bool) -> bool {
        true
    }

    pub fn current_focus(_app: &AppHandle) -> Option<Focus> {
        foreground()
    }

    /// Needs no AppHandle on Windows, so the manual test below can call it.
    pub(super) fn foreground() -> Option<Focus> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_invalid() {
            return None;
        }
        let mut exe = exe_path(process_of(window))?;
        // Store apps (Calculator, Settings, ...) run inside ApplicationFrameHost.exe;
        // the real app owns one of its child windows.
        if exe.to_lowercase().ends_with("\\applicationframehost.exe") {
            if let Some(hosted) = hosted_process(window).and_then(exe_path) {
                exe = hosted;
            }
        }
        let description = DESCRIPTIONS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(exe.clone())
            .or_insert_with(|| file_description(&exe))
            .clone();
        super::windows_focus(&exe, description, window_text(window))
    }

    fn process_of(window: HWND) -> u32 {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        pid
    }

    fn exe_path(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buffer = [0u16; 1024];
            let mut len = buffer.len() as u32;
            let result = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len);
            let _ = CloseHandle(process);
            result.ok()?;
            Some(String::from_utf16_lossy(&buffer[..len as usize]))
        }
    }

    /// The first child window that belongs to another process (a hosted Store app).
    fn hosted_process(frame: HWND) -> Option<u32> {
        struct Search {
            host: u32,
            found: Option<u32>,
        }
        unsafe extern "system" fn visit(child: HWND, lparam: LPARAM) -> BOOL {
            let search = &mut *(lparam.0 as *mut Search);
            let pid = process_of(child);
            if pid != 0 && pid != search.host {
                search.found = Some(pid);
                return BOOL(0);
            }
            BOOL(1)
        }
        let mut search = Search { host: process_of(frame), found: None };
        unsafe {
            let _ = EnumChildWindows(Some(frame), Some(visit), LPARAM(&mut search as *mut Search as isize));
        }
        search.found
    }

    fn window_text(window: HWND) -> Option<String> {
        let mut buffer = [0u16; 512];
        let len = unsafe { GetWindowTextW(window, &mut buffer) };
        (len > 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    }

    /// The exe's FileDescription ("Visual Studio Code"), in its first listed language.
    fn file_description(exe: &str) -> Option<String> {
        let path: Vec<u16> = exe.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
            // Translation is (language, code page) u16 pairs; its length is in bytes.
            let (pairs, bytes) = query_value(&data, "\\VarFileInfo\\Translation")?;
            if bytes < 4 {
                return None;
            }
            let (language, code_page) = (*pairs, *pairs.add(1));
            let key = format!("\\StringFileInfo\\{language:04x}{code_page:04x}\\FileDescription");
            // String values report their length in u16s.
            let (text, units) = query_value(&data, &key)?;
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(text, units as usize));
            let text = text.trim_end_matches('\0').trim();
            (!text.is_empty()).then(|| text.to_string())
        }
    }

    /// Pointer into `data` and the reported length of one version-resource value.
    unsafe fn query_value(data: &[u8], key: &str) -> Option<(*const u16, u32)> {
        let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
        let mut value = std::ptr::null_mut();
        let mut len = 0u32;
        let found = VerQueryValueW(data.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut value, &mut len);
        (found.as_bool() && !value.is_null() && len > 0).then_some((value as *const u16, len))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::Focus;
    use tauri::AppHandle;

    pub fn accessibility_trusted(_prompt: bool) -> bool {
        false
    }

    pub fn current_focus(_app: &AppHandle) -> Option<Focus> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> Recorder {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/005_app_sessions.sql")).unwrap();
        Recorder::new(conn)
    }

    fn focus(app: &str, title: &str) -> Option<Focus> {
        Some(Focus {
            app_name: app.into(),
            bundle_id: Some(format!("com.example.{app}")),
            window_title: Some(title.into()),
            document: None,
        })
    }

    fn sessions(recorder: &Recorder) -> Vec<AppSession> {
        sessions_since(&recorder.conn, 0).unwrap()
    }

    #[test]
    fn same_focus_extends_the_session() {
        let mut recorder = recorder();
        recorder.observe(focus("Code", "dijkstra.py"), 1_000).unwrap();
        recorder.observe(focus("Code", "dijkstra.py"), 3_000).unwrap();
        let rows = sessions(&recorder);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].started_at, rows[0].ended_at), (1_000, 3_000));
    }

    #[test]
    fn a_new_app_or_title_starts_a_new_session() {
        let mut recorder = recorder();
        recorder.observe(focus("Code", "dijkstra.py"), 1_000).unwrap();
        recorder.observe(focus("Code", "heap.py"), 3_000).unwrap();
        recorder.observe(focus("Chrome", "Two Sum - LeetCode"), 5_000).unwrap();
        let titles: Vec<_> = sessions(&recorder)
            .into_iter()
            .map(|session| session.window_title.unwrap())
            .collect();
        assert_eq!(titles, ["dijkstra.py", "heap.py", "Two Sum - LeetCode"]);
    }

    #[test]
    fn nothing_in_front_ends_the_session() {
        let mut recorder = recorder();
        recorder.observe(focus("Code", "dijkstra.py"), 1_000).unwrap();
        recorder.observe(None, 3_000).unwrap();
        recorder.observe(focus("Code", "dijkstra.py"), 5_000).unwrap();
        assert_eq!(sessions(&recorder).len(), 2);
    }

    #[test]
    fn private_apps_keep_only_the_name_and_titles_are_scrubbed() {
        let mut recorder = recorder();
        recorder
            .observe(
                Some(Focus {
                    app_name: "Messages".into(),
                    bundle_id: Some("com.apple.MobileSMS".into()),
                    window_title: Some("Chat with Sara".into()),
                    document: None,
                }),
                1_000,
            )
            .unwrap();
        recorder.observe(focus("Code", "notes for sara@example.com"), 3_000).unwrap();
        let rows = sessions(&recorder);
        assert_eq!(rows[0].window_title, None);
        assert!(!rows[1].window_title.as_deref().unwrap().contains('@'));
    }

    // macOS file URLs; on Windows Url::to_file_path needs a drive letter.
    #[cfg(target_os = "macos")]
    #[test]
    fn documents_keep_only_folder_and_file() {
        assert_eq!(
            short_document("file:///Users/sultan/school/cs170-hw4/dijkstra.py").as_deref(),
            Some("cs170-hw4/dijkstra.py")
        );
        assert_eq!(short_document("not a url"), None);
    }

    #[test]
    fn windows_focus_names_the_app_and_keeps_its_title() {
        let focus = windows_focus(
            r"C:\Users\sam\AppData\Local\Programs\Microsoft VS Code\Code.exe",
            Some("Visual Studio Code ".into()),
            Some("dijkstra.py - cs170-hw4 - Visual Studio Code".into()),
        )
        .unwrap();
        assert_eq!(focus.app_name, "Visual Studio Code");
        assert_eq!(focus.bundle_id.as_deref(), Some("code.exe"));
        assert_eq!(focus.window_title.as_deref(), Some("dijkstra.py - cs170-hw4 - Visual Studio Code"));

        let no_description = windows_focus(r"C:\Windows\notepad.exe", None, None).unwrap();
        assert_eq!(no_description.app_name, "notepad");
    }

    #[test]
    fn windows_browser_titles_are_never_recorded() {
        let focus = windows_focus(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            Some("Google Chrome".into()),
            Some("Secret tab - Google Chrome (Incognito)".into()),
        )
        .unwrap();
        assert_eq!(focus.app_name, "Google Chrome");
        assert_eq!(focus.window_title, None);
    }

    #[test]
    fn windows_skips_our_own_app_and_blanks_private_apps() {
        assert_eq!(windows_focus(r"C:\work-match\target\debug\work_match.exe", None, None), None);
        assert_eq!(windows_focus(r"C:\Program Files\work-match 2\work-match 2.exe", None, None), None);

        let outlook = windows_focus(
            r"C:\Program Files\WindowsApps\Microsoft.OutlookForWindows\olk.exe",
            Some("Microsoft Outlook".into()),
            Some("Inbox - sam@example.com - Outlook".into()),
        )
        .unwrap()
        .cleaned();
        assert_eq!(outlook.window_title, None);
    }

    /// Manual check of the Win32 calls against whatever window is in front:
    ///   cargo test foreground_app_is_readable -- --ignored --nocapture
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn foreground_app_is_readable() {
        let focus = platform::foreground().expect("nothing in front (or our own app)");
        println!(
            "in front: {} ({:?}), title: {} chars",
            focus.app_name,
            focus.bundle_id,
            focus.window_title.as_deref().map_or(0, |title| title.chars().count())
        );
        assert!(!focus.app_name.is_empty());
    }

    #[test]
    fn sessions_since_skips_old_ones() {
        let mut recorder = recorder();
        recorder.observe(focus("Code", "old.py"), 1_000).unwrap();
        recorder.observe(focus("Code", "new.py"), 600_000).unwrap();
        let recent = sessions_since(&recorder.conn, 300_000).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].window_title.as_deref(), Some("new.py"));
    }
}
