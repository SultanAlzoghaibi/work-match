# wm-ocr

Small Swift command-line tool that captures the frontmost window and reads its
text with Apple's Vision OCR, all on-device. The Tauri app runs it about once a
minute and feeds the text to the local summarizer (see `docs/SPEC.md` §3.1).

The screenshot stays in memory and is never written to disk. Requires macOS 14+.

## Build

```sh
./build.sh          # -> bin/wm-ocr-<target-triple>, the name Tauri sidecars need
```

## Test

```sh
./test.sh   # builds, then checks columns, secret redaction and errors on rendered test images
```

No Screen Recording permission is needed: the checks run on images, not your screen.

## Use

```sh
wm-ocr                                   # capture the frontmost window
wm-ocr --no-correction                   # recommended: language correction mangles code
wm-ocr --fast                            # ~10x faster, less accurate
wm-ocr --skip-bundle com.apple.MobileSMS,com.apple.mail   # blocklist, checked before capture
wm-ocr --image shot.png                  # OCR a file instead (testing, no permission needed)
wm-ocr --request-permission              # trigger the Screen Recording prompt
```

Output (one JSON object on stdout):

```json
{
  "app": "Code",
  "bundle_id": "com.microsoft.VSCode",
  "window_title": "dijkstra.py — cs170-hw4",
  "source": "window",
  "width": 2880, "height": 1800,
  "lines": [{"text": "def dijkstra(graph, src):", "confidence": 1, "x": 0.03, "y": 0.12, "w": 0.3, "h": 0.03}],
  "main_text": "import heapq\ndef dijkstra(graph, src):\n...",
  "skipped": false,
  "elapsed_ms": 228
}
```

- `lines`: every recognized line, with position as fractions of the image (top-left origin).
- `columns`: side-by-side text areas (sidebar, editor, chat panel), largest first, each read top to bottom.
- `main_text`: the columns joined in that order, without low-confidence noise or the title/status bar bands. Send this to the summarizer.
- `skipped: true`: nothing was captured. `skip_reason` is `blocklist` (the app was in `--skip-bundle`) or `sensitive_window` (the title looks like `.env`, `credentials`, `id_rsa`, a `.pem` file, a keychain or a password manager).
- `redactions`: how many secrets were replaced with `[redacted]`.

## Secrets

Before anything is printed, URL passwords (`postgresql://user:pw@...`), `password=` / `token:` / `api_key=` style values, private key headers, and known token shapes (OpenAI/OpenRouter `sk-`, Supabase `sb_secret_`, AWS `AKIA`, GitHub `ghp_`, Slack `xox`, Google `AIza`, JWTs, and long random strings) are replaced with `[redacted]`. Tested on an image of fake secrets: all replaced, normal code and URLs left alone.

Errors print `{"error": "...", "message": "..."}` and exit non-zero:
`1` bad arguments/image, `2` no Screen Recording permission, `3` no window, `4` capture/OCR failed.

## Measured (M-series, macOS 26)

| Input | Mode | Time |
|---|---|---|
| Photo, 1512×2016 | accurate | ~0.9 s (first run includes model load) |
| Photo, 1512×2016 | fast | ~70 ms |
| Code screenshot, 1400×800 | accurate, `--no-correction` | ~0.2 s |

## Permission

macOS asks for **Screen Recording** once (System Settings → Privacy &
Security). The permission belongs to the app that launches `wm-ocr`: your
terminal while testing, the work-match app once bundled.

## Hooking it into Tauri (not done yet)

1. Add the sidecar to `src-tauri/tauri.conf.json`:
   `"bundle": { "externalBin": ["../ocr-helper/bin/wm-ocr"] }`
2. Add `tauri-plugin-shell` and run it from Rust:
   ```rust
   let out = app.shell().sidecar("wm-ocr")?
       .args(["--no-correction", "--skip-bundle", &blocklist])
       .output().await?;
   let ocr: serde_json::Value = serde_json::from_slice(&out.stdout)?;
   ```
3. Keep `main_text` in memory with the app name and window title, and summarize every 5 minutes.
