fn main() {
    ensure_sidecar_exists("llama-server");
    tauri_build::build()
}

/// tauri-build refuses to build when an externalBin file is missing, and the
/// llama-server sidecar isn't committed (see binaries/README.md). An empty
/// stand-in keeps the app building; at runtime llm.rs sees it is empty, logs
/// it, and summaries use the fallback.
fn ensure_sidecar_exists(name: &str) {
    let target = std::env::var("TARGET").unwrap();
    let exe = if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::PathBuf::from(format!("binaries/{name}-{target}{exe}"));
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 0) {
        return;
    }
    println!(
        "cargo:warning={} is missing or an empty placeholder: summaries will use the fallback (see src-tauri/binaries/README.md)",
        path.display()
    );
    if !path.exists() {
        std::fs::create_dir_all("binaries").unwrap();
        std::fs::write(&path, b"").unwrap();
    }
}
