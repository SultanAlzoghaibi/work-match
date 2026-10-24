#!/bin/sh
# Builds wm-ocr and runs the end-to-end checks in tests/check.swift.
# Uses rendered test images only; no Screen Recording permission needed.
set -e
cd "$(dirname "$0")"
swift build -c release
swift tests/check.swift .build/release/wm-ocr
