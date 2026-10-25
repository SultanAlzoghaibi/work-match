// The llama.cpp `llama-server` sidecar that runs Llama 3.2 on this machine,
// and the client the summarizer calls it with. The server listens on
// 127.0.0.1 only and requires a per-launch API key: by default it allows any
// web origin, so without the key any page in the browser could drive it.
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tauri::async_runtime::Receiver;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::watch;

pub const PORT: u16 = 8089;
/// Preferred first; the 1B model is the lighter fallback.
const MODEL_FILES: [&str; 2] = [
    "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
    "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
];
const PID_FILE: &str = "llama-server.pid";
// A long task_summary takes a while on a laptop CPU/GPU.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_POLL: Duration = Duration::from_millis(500);
const STARTUP_LOG_LINES: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    Starting,
    Ready,
    Down,
}

#[derive(Clone)]
pub struct Llm(Arc<Inner>);

struct Inner {
    client: reqwest::Client,
    api_key: String,
    status: watch::Sender<Status>,
    child: Mutex<Option<CommandChild>>,
    pid_file: Option<PathBuf>,
}

#[derive(Deserialize)]
struct ChatReply {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

/// Starts the sidecar if a model and the real binary are present. Without
/// them the returned client stays down and every summary uses the fallback.
pub fn start(app: &AppHandle) -> Llm {
    let data_dir = app.path().app_data_dir().ok();
    let llm = Llm::new(data_dir.as_ref().map(|dir| dir.join(PID_FILE)));

    let model_override = std::env::var_os("LLAMA_MODEL_PATH").map(PathBuf::from);
    let models_dir = data_dir.as_ref().map(|dir| dir.join("models"));
    let Some(model) = find_model(model_override, models_dir.as_deref()) else {
        eprintln!(
            "[llm] no model: put {} (or the 1B file) in {} or set LLAMA_MODEL_PATH; summaries will use the fallback",
            MODEL_FILES[0],
            models_dir.map_or("<app data>/models".into(), |dir| dir.display().to_string()),
        );
        return llm;
    };
    if !sidecar_is_real() {
        eprintln!("[llm] llama-server sidecar is missing or the build placeholder (see src-tauri/binaries/README.md); summaries will use the fallback");
        return llm;
    }

    if let Some(pid_file) = &llm.0.pid_file {
        kill_orphan(pid_file);
    }
    let args = server_args(&model, &llm.0.api_key, half_the_cores());
    let spawned = app
        .shell()
        .sidecar("llama-server")
        .and_then(|command| command.args(args).spawn());
    let (events, child) = match spawned {
        Ok(spawned) => spawned,
        Err(err) => {
            eprintln!("[llm] could not start llama-server: {err}; summaries will use the fallback");
            return llm;
        }
    };
    tie_to_app(child.pid());
    if let Some(pid_file) = &llm.0.pid_file {
        if let Err(err) = std::fs::write(pid_file, child.pid().to_string()) {
            eprintln!("[llm] could not write {}: {err}", pid_file.display());
        }
    }
    *llm.child() = Some(child);
    llm.0.status.send_replace(Status::Starting);
    println!(
        "[llm] starting llama-server with {}",
        model.file_name().unwrap_or_default().to_string_lossy()
    );

    tauri::async_runtime::spawn(watch_events(events, llm.clone()));
    tauri::async_runtime::spawn(wait_until_healthy(llm.clone()));
    llm
}

impl Llm {
    fn new(pid_file: Option<PathBuf>) -> Self {
        let client = reqwest::Client::builder()
            // A proxy for 127.0.0.1 would see the page text in every prompt.
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("http client");
        Self(Arc::new(Inner {
            client,
            api_key: random_api_key(),
            status: watch::Sender::new(Status::Down),
            child: Mutex::new(None),
            pid_file,
        }))
    }

    fn status(&self) -> Status {
        *self.0.status.borrow()
    }

    /// For the Dev tab: "ready", "starting" or "down".
    pub fn status_label(&self) -> &'static str {
        match self.status() {
            Status::Ready => "ready",
            Status::Starting => "starting",
            Status::Down => "down",
        }
    }

    fn child(&self) -> std::sync::MutexGuard<'_, Option<CommandChild>> {
        self.0.child.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// True once /health answers OK; false if the server is down or not ready in time.
    pub async fn wait_ready(&self, timeout: Duration) -> bool {
        let mut status = self.0.status.subscribe();
        let settled = tokio::time::timeout(timeout, status.wait_for(|status| *status != Status::Starting)).await;
        matches!(settled, Ok(Ok(status)) if *status == Status::Ready)
    }

    /// One chat completion constrained to `schema`; returns the reply text.
    /// Errors describe what failed, never the prompt or the reply.
    pub async fn chat(&self, system: &str, user: &str, schema: &Value) -> Result<String, String> {
        if self.status() != Status::Ready {
            return Err("llama-server is not running".into());
        }
        let body = json!({
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "temperature": 0,
            // Room for a task_summary of up to ~500 words plus the short fields;
            // still fits the 4096-token context (-c) with the prompt.
            "max_tokens": 1000,
            "stream": false,
            "response_format": { "type": "json_object", "schema": schema },
        });
        let response = self
            .0
            .client
            .post(format!("http://127.0.0.1:{PORT}/v1/chat/completions"))
            .bearer_auth(&self.0.api_key)
            .json(&body)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|err| format!("request failed: {err}"))?;
        let reply: ChatReply = response
            .json()
            .await
            .map_err(|err| format!("unreadable reply: {err}"))?;
        reply
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| "reply had no content".into())
    }

    pub fn shutdown(&self) {
        self.0.status.send_replace(Status::Down);
        let Some(child) = self.child().take() else {
            return;
        };
        match child.kill() {
            Ok(()) => {
                if let Some(pid_file) = &self.0.pid_file {
                    let _ = std::fs::remove_file(pid_file);
                }
            }
            // The pid file stays, so the next launch can stop it.
            Err(err) => eprintln!("[llm] stopping llama-server failed: {err}"),
        }
    }
}

fn find_model(model_override: Option<PathBuf>, models_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = model_override {
        return path.is_file().then_some(path);
    }
    let models_dir = models_dir?;
    MODEL_FILES
        .iter()
        .map(|file| models_dir.join(file))
        .find(|path| path.is_file())
}

/// build.rs stands in an empty file when binaries/llama-server-<triple> is
/// missing, so the app still builds without the sidecar.
fn sidecar_is_real() -> bool {
    let sidecar = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(format!("llama-server{}", std::env::consts::EXE_SUFFIX))));
    sidecar
        .and_then(|path| std::fs::metadata(path).ok())
        .is_some_and(|meta| meta.len() > 0)
}

fn server_args(model: &Path, api_key: &str, threads: usize) -> Vec<String> {
    vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        PORT.to_string(),
        "-c".into(),
        "4096".into(),
        "-t".into(),
        threads.to_string(),
        // One slot: the default splits the context across several.
        "-np".into(),
        "1".into(),
        "--api-key".into(),
        api_key.into(),
        "--no-webui".into(),
        // /slots would expose server state to anything that can reach the port.
        "--no-slots".into(),
    ]
}

fn half_the_cores() -> usize {
    std::thread::available_parallelism()
        .map_or(2, |cores| cores.get() / 2)
        .max(1)
}

fn random_api_key() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS random number generator");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The event channel holds one message and the reader threads block on it, so
/// output must be drained for the server's whole life or it stalls. Output is
/// only kept until /health is OK (printed if the server dies first); after
/// that it could echo prompt text, so it is dropped unread.
async fn watch_events(mut events: Receiver<CommandEvent>, llm: Llm) {
    let mut startup_log = VecDeque::with_capacity(STARTUP_LOG_LINES);
    while let Some(event) = events.recv().await {
        match event {
            CommandEvent::Stdout(line) | CommandEvent::Stderr(line) => {
                if llm.status() == Status::Starting {
                    if startup_log.len() == STARTUP_LOG_LINES {
                        startup_log.pop_front();
                    }
                    startup_log.push_back(String::from_utf8_lossy(&line).trim_end().to_string());
                }
            }
            CommandEvent::Terminated(payload) => {
                let was = llm.0.status.send_replace(Status::Down);
                if was == Status::Starting {
                    for line in &startup_log {
                        eprintln!("[llama-server] {line}");
                    }
                }
                // Down already means shutdown() stopped it on purpose.
                if was != Status::Down {
                    eprintln!(
                        "[llm] llama-server exited (code {:?}); summaries will use the fallback",
                        payload.code
                    );
                }
            }
            CommandEvent::Error(err) => eprintln!("[llm] llama-server: {err}"),
            _ => {}
        }
    }
}

async fn wait_until_healthy(llm: Llm) {
    let health = format!("http://127.0.0.1:{PORT}/health");
    while llm.status() == Status::Starting {
        let ok = llm
            .0
            .client
            .get(&health)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if ok {
            // Only Starting becomes Ready: the process may have exited meanwhile.
            let became_ready = llm.0.status.send_if_modified(|status| {
                let starting = *status == Status::Starting;
                if starting {
                    *status = Status::Ready;
                }
                starting
            });
            if became_ready {
                println!("[llm] llama-server ready");
            }
            return;
        }
        tokio::time::sleep(HEALTH_POLL).await;
    }
}

/// `tauri dev` SIGKILLs the app on every rebuild, which orphans the sidecar and
/// leaves it holding the port. Stop a server left over from an earlier run.
#[cfg(unix)]
fn kill_orphan(pid_file: &Path) {
    use std::process::Command;
    let Some(pid) = std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
    else {
        return;
    };
    let _ = std::fs::remove_file(pid_file);
    let pid = pid.to_string();
    // The pid may belong to an unrelated process by now; only stop a llama-server.
    let Ok(ps) = Command::new("ps").args(["-p", &pid, "-o", "comm="]).output() else {
        return;
    };
    if !String::from_utf8_lossy(&ps.stdout).contains("llama-server") {
        return;
    }
    if Command::new("kill").args(["-9", &pid]).status().is_ok_and(|status| status.success()) {
        eprintln!("[llm] stopped a llama-server left over from an earlier run (pid {pid})");
        // Let the kernel release the port before the new server binds it.
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[cfg(not(unix))]
fn kill_orphan(_pid_file: &Path) {}

/// Windows has no process groups, so an app that's killed or crashes (`tauri dev`
/// kills it on every rebuild) would leave llama-server holding the port. A job
/// object that kills its processes when its last handle closes ties the server
/// to this process instead, so no pid-file cleanup is needed.
#[cfg(windows)]
fn tie_to_app(pid: u32) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    let tied = unsafe {
        (|| -> windows::core::Result<()> {
            // Never closed on purpose: Windows closes it when this process exits,
            // and that kills the server.
            let job = CreateJobObjectW(None, PCWSTR::null())?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)?;
            let assigned = AssignProcessToJobObject(job, process);
            let _ = CloseHandle(process);
            assigned
        })()
    };
    if let Err(err) = tied {
        eprintln!("[llm] could not tie llama-server to the app ({err}); it may outlive a crash");
    }
}

#[cfg(not(windows))]
fn tie_to_app(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_listens_only_on_loopback_with_a_key() {
        let args = server_args(Path::new("/models/m.gguf"), "secret", 4);
        let joined = args.join(" ");
        assert!(joined.contains("--host 127.0.0.1 --port 8089"));
        assert!(joined.contains("-c 4096 -t 4 -np 1"));
        assert!(joined.contains("--api-key secret"));
        assert!(args.contains(&"--no-webui".to_string()));
        assert!(args.contains(&"--no-slots".to_string()));
        assert!(!joined.contains("0.0.0.0"));
    }

    #[test]
    fn api_keys_are_random_hex() {
        let key = random_api_key();
        assert_eq!(key.len(), 32);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(key, random_api_key());
    }

    #[test]
    fn finds_the_3b_model_before_the_1b_one() {
        let dir = std::env::temp_dir().join(format!("work-match-models-{}", random_api_key()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_model(None, Some(&dir)), None);

        std::fs::write(dir.join(MODEL_FILES[1]), b"1b").unwrap();
        assert_eq!(find_model(None, Some(&dir)), Some(dir.join(MODEL_FILES[1])));

        std::fs::write(dir.join(MODEL_FILES[0]), b"3b").unwrap();
        assert_eq!(find_model(None, Some(&dir)), Some(dir.join(MODEL_FILES[0])));

        // An explicit override wins, and a missing one isn't silently replaced.
        let custom = dir.join("custom.gguf");
        assert_eq!(find_model(Some(custom.clone()), Some(&dir)), None);
        std::fs::write(&custom, b"custom").unwrap();
        assert_eq!(find_model(Some(custom.clone()), Some(&dir)), Some(custom));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn chat_fails_fast_while_the_server_is_down() {
        let llm = Llm::new(None);
        assert!(!llm.wait_ready(Duration::from_millis(10)).await);
        let err = llm.chat("system", "user", &json!({})).await.unwrap_err();
        assert_eq!(err, "llama-server is not running");
    }
}
