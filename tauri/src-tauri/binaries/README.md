# Sidecar binaries

`llama-server` from [llama.cpp](https://github.com/ggml-org/llama.cpp) runs Llama 3.2 on
the user's machine for the activity summarizer (`src/llm.rs`). Tauri bundles it as a
sidecar (`bundle.externalBin` in `tauri.conf.json`), so it has to be in this folder as
`llama-server-<target-triple>`:

| Platform | File |
|---|---|
| macOS, Apple Silicon | `llama-server-aarch64-apple-darwin` |
| Windows x64 | `llama-server-x86_64-pc-windows-msvc.exe` |

The binaries are gitignored. When the file is missing, `build.rs` puts an empty
placeholder here, so the app still builds and prints a cargo warning. At runtime the app
logs that llama-server can't start, and every summary comes back as `source: "fallback"`.

## Version

llama.cpp tag **b11669** (released 2026-10-15). The flags the app passes (`-m --host
--port -c -t -np --api-key --no-webui --no-slots`) were checked against that tag's
`tools/server/README.md`, and so were `/health` and the `/v1/chat/completions` field
`response_format: {"type": "json_object", "schema": …}`.

## macOS (Apple Silicon): build it

The release tarball (`llama-b11669-bin-macos-arm64.tar.gz`) ships `llama-server` together
with `libllama`/`libggml*.dylib`, which it loads via `@loader_path`. A sidecar has to be a
single file, so build a static one instead:

```sh
xcode-select --install       # once
brew install cmake           # once
sh build-llama-server.sh     # -> llama-server-aarch64-apple-darwin
```

The script clones tag b11669 (override with `LLAMA_TAG=...`), builds only `llama-server`
with `BUILD_SHARED_LIBS=OFF` and the Metal shaders embedded, and prints `otool -L`. The
output should list only system libraries.

## Windows x64: build it

The release zip (`llama-b11669-bin-win-cpu-x64.zip`) splits `llama-server.exe` into a
small launcher plus `llama`/`ggml`/OpenMP DLLs. A sidecar has to be a single file, so build
a static one instead. You need Visual Studio 2022 with **Desktop development with C++**
(the script uses its bundled cmake if cmake isn't on PATH) and git:

```powershell
powershell -ExecutionPolicy Bypass -File build-llama-server.ps1   # -> llama-server-x86_64-pc-windows-msvc.exe
```

The script clones tag b11669 into `%TEMP%` (override with `$env:LLAMA_TAG`), builds only
`llama-server` with `BUILD_SHARED_LIBS=OFF`, the static C runtime and OpenMP off, and
prints `dumpbin /dependents`. The output should list only Windows system DLLs. Like the
macOS build, it targets the CPU it's built on, so build it on each demo laptop rather than
copying the file between machines.

On Windows the app puts llama-server in a job object, so it stops whenever the app exits,
crashes or is restarted by `tauri dev`.

## After replacing a binary

Cargo only notices the new file if its modification time is newer than the last build.
Explorer, `Copy-Item` and `cp -p` keep the old timestamp of the downloaded file, so the app
would keep running the old copy (or the placeholder). Touch the file after copying it:

```sh
touch binaries/llama-server-aarch64-apple-darwin                                    # macOS
(Get-Item binaries\llama-server-x86_64-pc-windows-msvc.exe).LastWriteTime = Get-Date  # PowerShell
```

`build-llama-server.sh` and `build-llama-server.ps1` already give their output a fresh timestamp.

## Model

The model isn't downloaded automatically. Put one of these in the app data folder under
`models/`:

| Model | Size | Download |
|---|---|---|
| Llama 3.2 3B Instruct, Q4_K_M (preferred) | 2.0 GB | [Llama-3.2-3B-Instruct-Q4_K_M.gguf](https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf) |
| Llama 3.2 1B Instruct, Q4_K_M (lighter fallback) | 0.8 GB | [Llama-3.2-1B-Instruct-Q4_K_M.gguf](https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf) |

- macOS: `~/Library/Application Support/com.sultanalzoghaibi.workmatch/models/`
- Windows: `%APPDATA%\com.sultanalzoghaibi.workmatch\models\`

If both are there, the 3B model is used. To use any other GGUF file, set
`LLAMA_MODEL_PATH`.
