#!/bin/sh
# Builds wm-ocr and copies it to bin/ with the target-triple suffix Tauri
# expects for a sidecar (bundle.externalBin), e.g. bin/wm-ocr-aarch64-apple-darwin.
set -e
cd "$(dirname "$0")"
swift build -c release
triple="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
triple="${triple:-$(uname -m | sed 's/arm64/aarch64/')-apple-darwin}"
mkdir -p bin
cp .build/release/wm-ocr "bin/wm-ocr-$triple"
cp .build/release/wm-ocr "../src-tauri/binaries/wm-ocr-$triple"   # the app's sidecar (screen_capture.rs)
echo "built bin/wm-ocr-$triple and src-tauri/binaries/wm-ocr-$triple"
