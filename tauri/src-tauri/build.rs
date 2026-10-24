fn main() {
    ensure_sidecar_exists("llama-server");
    ensure_sidecar_exists("wm-ocr");
    tauri_build::build()
}

/// tauri-build refuses to build when an externalBin file is missing, and the
/// sidecars aren't committed (see binaries/README.md). An empty stand-in keeps
/// the app building; at runtime llm.rs / screen_capture.rs see it is empty,
/// log it, and carry on without it.
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
        "cargo:warning={} is missing or an empty placeholder (see src-tauri/binaries/README.md)",
        path.display()
    );
    if !path.exists() {
        std::fs::create_dir_all("binaries").unwrap();
        std::fs::write(&path, b"").unwrap();
    }
}
