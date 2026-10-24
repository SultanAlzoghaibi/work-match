// wm-ocr: capture the frontmost window and read its text with Apple's Vision
// OCR, all on-device. Prints one JSON object to stdout. The screenshot only
// ever exists in memory; nothing is written to disk.
//
//   wm-ocr                         capture the frontmost window
//   wm-ocr --image shot.png        OCR an image file instead (for testing)
//   wm-ocr --fast                  faster, less accurate recognition
//   wm-ocr --no-correction         no language correction (better for code)
//   wm-ocr --skip-bundle a,b       skip if the frontmost app is one of these
//   wm-ocr --max-lines 200         cap the number of lines returned
//   wm-ocr --request-permission    ask macOS for Screen Recording access
//
// Secrets (URL passwords, API keys, tokens, private keys) are replaced with
// [redacted] before printing, and windows titled like .env / credentials /
// id_rsa are never captured.
//
// Exit codes: 0 ok (or skipped), 1 bad arguments, 2 no permission,
// 3 no window to capture, 4 capture/OCR failed.

import AppKit
import CoreGraphics
import Foundation
import ScreenCaptureKit
import Vision

// MARK: - Output

struct Line: Encodable {
    let text: String
    let confidence: Float
    // Top-left origin, fractions of the image size (0...1).
    let x: Double
    let y: Double
    let w: Double
    let h: Double
}

struct Result: Encodable {
    var app: String?
    var bundleId: String?
    var windowTitle: String?
    var source: String           // "window" | "image"
    var width: Int = 0
    var height: Int = 0
    var lines: [Line] = []
    var columns: [Column] = []   // side-by-side text areas, largest first
    var mainText: String = ""    // columns in reading order, chrome stripped
    var skipped: Bool = false
    var skipReason: String?      // "blocklist" | "sensitive_window"
    var redactions: Int = 0      // secrets replaced with [redacted]
    var elapsedMs: Int = 0

    enum CodingKeys: String, CodingKey {
        case app, source, width, height, lines, columns, skipped
        case bundleId = "bundle_id"
        case windowTitle = "window_title"
        case mainText = "main_text"
        case skipReason = "skip_reason"
        case redactions
        case elapsedMs = "elapsed_ms"
    }
}

struct Failure: Encodable {
    let error: String
    let message: String
}

func printJSON<T: Encodable>(_ value: T) {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    let data = (try? encoder.encode(value)) ?? Data("{}".utf8)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

func fail(_ code: Int32, _ error: String, _ message: String) -> Never {
    printJSON(Failure(error: error, message: message))
    exit(code)
}

// MARK: - Arguments

struct Options {
    var imagePath: String?
    var fast = false
    var correction = true
    var skipBundles: Set<String> = []
    var maxLines = 400
    var requestPermission = false
}

func parseOptions() -> Options {
    var opts = Options()
    var args = CommandLine.arguments.dropFirst().makeIterator()
    while let arg = args.next() {
        switch arg {
        case "--image":
            guard let path = args.next() else { fail(1, "bad_args", "--image needs a path") }
            opts.imagePath = path
        case "--fast":
            opts.fast = true
        case "--no-correction":
            opts.correction = false
        case "--skip-bundle":
            guard let list = args.next() else { fail(1, "bad_args", "--skip-bundle needs a list") }
            opts.skipBundles = Set(list.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) })
        case "--max-lines":
            guard let n = args.next().flatMap(Int.init), n > 0 else { fail(1, "bad_args", "--max-lines needs a number") }
            opts.maxLines = n
        case "--request-permission":
            opts.requestPermission = true
        case "-h", "--help":
            print("usage: wm-ocr [--image PATH] [--fast] [--no-correction] [--skip-bundle a,b] [--max-lines N] [--request-permission]")
            exit(0)
        default:
            fail(1, "bad_args", "unknown argument \(arg)")
        }
    }
    return opts
}

// MARK: - Capture

/// The frontmost app's frontmost normal window, using the window server's
/// front-to-back order (SCShareableContent's order is not z-order).
func frontmostWindow(content: SCShareableContent, pid: pid_t) -> SCWindow? {
    let byId = Dictionary(content.windows.map { ($0.windowID, $0) }, uniquingKeysWith: { a, _ in a })
    let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
        as? [[String: Any]] ?? []
    for entry in info {
        guard (entry[kCGWindowOwnerPID as String] as? pid_t) == pid,
              (entry[kCGWindowLayer as String] as? Int) == 0,
              let number = entry[kCGWindowNumber as String] as? UInt32,
              let window = byId[number],
              window.frame.width > 50, window.frame.height > 50
        else { continue }
        return window
    }
    return nil
}

func captureFrontmostWindow(opts: Options, result: inout Result) async -> CGImage? {
    if opts.requestPermission && !CGPreflightScreenCaptureAccess() {
        _ = CGRequestScreenCaptureAccess()
    }
    guard CGPreflightScreenCaptureAccess() else {
        fail(2, "screen_recording_permission",
             "Grant Screen Recording in System Settings > Privacy & Security, then relaunch.")
    }
    guard let front = NSWorkspace.shared.frontmostApplication else {
        fail(3, "no_window", "No frontmost application")
    }
    result.app = front.localizedName
    result.bundleId = front.bundleIdentifier

    // Blocklist is checked before anything is captured.
    if let bundle = front.bundleIdentifier, opts.skipBundles.contains(bundle) {
        result.skipped = true
        result.skipReason = "blocklist"
        return nil
    }

    do {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        guard let window = frontmostWindow(content: content, pid: front.processIdentifier) else {
            fail(3, "no_window", "\(front.localizedName ?? "App") has no visible window")
        }
        result.windowTitle = window.title
        if isSensitive(title: window.title) {
            result.skipped = true
            result.skipReason = "sensitive_window"
            return nil
        }

        let filter = SCContentFilter(desktopIndependentWindow: window)
        let config = SCStreamConfiguration()
        let scale = CGFloat(filter.pointPixelScale)
        config.width = Int(window.frame.width * scale)
        config.height = Int(window.frame.height * scale)
        config.showsCursor = false
        return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
    } catch {
        fail(4, "capture_failed", error.localizedDescription)
    }
}

func loadImage(path: String) -> CGImage {
    let url = URL(fileURLWithPath: path)
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { fail(1, "bad_image", "Could not read image at \(path)") }
    return image
}

// MARK: - OCR

func recognizeText(in image: CGImage, fast: Bool, correction: Bool) throws -> [Line] {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = fast ? .fast : .accurate
    request.usesLanguageCorrection = !fast && correction
    try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])

    return (request.results ?? []).compactMap { observation in
        guard let candidate = observation.topCandidates(1).first else { return nil }
        let box = observation.boundingBox  // bottom-left origin
        return Line(
            text: candidate.string,
            confidence: candidate.confidence,
            x: box.minX, y: 1 - box.maxY, w: box.width, h: box.height
        )
    }
}

// MARK: - Secrets

/// Window titles that usually mean secrets on screen. These windows are never
/// captured at all.
let sensitiveTitle = try! NSRegularExpression(
    pattern: #"(^|[\s/\\])\.env(\.\w+)?\b|credentials|secrets?\.(json|ya?ml|toml)|id_(rsa|ed25519|ecdsa)|\.pem\b|\.p12\b|\.keystore\b|keychain|password"#,
    options: [.caseInsensitive]
)

func isSensitive(title: String?) -> Bool {
    guard let title else { return false }
    return sensitiveTitle.firstMatch(in: title, range: NSRange(title.startIndex..., in: title)) != nil
}

/// Patterns for secrets that can still show up in normal windows (a terminal,
/// a config file, a chat). Each match is replaced before the text leaves the
/// helper, so the summarizer and the "What work-match sees" panel never get it.
let secretPatterns: [(NSRegularExpression, String)] = [
    // user:password@ inside any URL, e.g. postgresql://user:pw@host
    (#"\b([a-z][a-z0-9+.\-]*://)[^\s/@]+:[^\s/]+@"#, "$1[redacted]@"),
    // key = value / key: value for secret-looking keys
    (#"(?i)\b([\w.\-]*(?:password|passwd|pwd|secret|token|api[_\-]?key|access[_\-]?key|private[_\-]?key|auth)[\w.\-]*)(\s*[:=]\s*)\S+"#, "$1$2[redacted]"),
    (#"-----BEGIN [A-Z ]*PRIVATE KEY-----"#, "[redacted private key]"),
    // Known token shapes
    (#"\bsk-[A-Za-z0-9_\-]{16,}"#, "[redacted]"),
    (#"\bsb_(?:secret|publishable)_[A-Za-z0-9_\-]{10,}"#, "[redacted]"),
    (#"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"#, "[redacted]"),
    (#"\bgh[pousr]_[A-Za-z0-9]{20,}"#, "[redacted]"),
    (#"\bxox[abprs]-[A-Za-z0-9\-]{10,}"#, "[redacted]"),
    (#"\bAIza[0-9A-Za-z_\-]{30,}"#, "[redacted]"),
    (#"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{4,}"#, "[redacted]"),  // JWT
    // Long random-looking strings: 32+ chars mixing letters and digits
    (#"\b(?=[A-Za-z0-9+/_\-]*\d)(?=[A-Za-z0-9+/_\-]*[A-Za-z])[A-Za-z0-9+/_\-]{32,}\b"#, "[redacted]"),
].map { (try! NSRegularExpression(pattern: $0.0), $0.1) }

/// Returns the text with secrets replaced, and how many were replaced.
func redact(_ text: String) -> (String, Int) {
    var out = text
    var count = 0
    for (regex, template) in secretPatterns {
        let range = NSRange(out.startIndex..., in: out)
        count += regex.numberOfMatches(in: out, range: range)
        out = regex.stringByReplacingMatches(in: out, range: range, withTemplate: template)
    }
    return (out, count)
}

struct Column: Encodable {
    let x: Double
    let w: Double
    let text: String
}

/// Lines worth reading: drops low-confidence noise and the thin bands where
/// title bars and status bars sit.
func readable(_ lines: [Line]) -> [Line] {
    lines.filter { $0.confidence >= 0.5 && $0.y > 0.03 && $0.y + $0.h < 0.97 }
}

/// Splits the window into side-by-side columns (sidebar, editor, chat panel)
/// at vertical gutters: x ranges that no line covers. Lines wider than half
/// the window (titles, banners) would bridge the gutters, so they don't shape
/// the columns; each line then joins the column it overlaps most. Columns
/// come back largest first, so the main content leads.
func columns(from lines: [Line], minGutter: Double = 0.02) -> [Column] {
    let shaping = lines.filter { $0.w < 0.5 }
    let spans = (shaping.isEmpty ? lines : shaping).map { ($0.x, $0.x + $0.w) }.sorted { $0.0 < $1.0 }

    var bands: [(lo: Double, hi: Double)] = []
    for (lo, hi) in spans {
        if let last = bands.last, lo - last.hi < minGutter {
            bands[bands.count - 1].hi = max(last.hi, hi)
        } else {
            bands.append((lo, hi))
        }
    }
    guard !bands.isEmpty else { return [] }

    var members = Array(repeating: [Line](), count: bands.count)
    for line in lines {
        let overlaps = bands.map { min($0.hi, line.x + line.w) - max($0.lo, line.x) }
        let best = overlaps.indices.max { overlaps[$0] < overlaps[$1] } ?? 0
        members[best].append(line)
    }

    return zip(bands, members)
        .filter { !$0.1.isEmpty }
        .map { band, lines -> (Column, Double) in
            let ordered = lines.sorted { abs($0.y - $1.y) > 0.005 ? $0.y < $1.y : $0.x < $1.x }
            let area = lines.reduce(0) { $0 + $1.w * $1.h }
            return (Column(x: band.lo, w: band.hi - band.lo, text: ordered.map(\.text).joined(separator: "\n")), area)
        }
        .sorted { $0.1 > $1.1 }
        .map(\.0)
}

/// Text for the summarizer: each column read top to bottom, largest first.
func mainText(from columns: [Column]) -> String {
    columns.map(\.text).joined(separator: "\n\n")
}

// MARK: - Main

let opts = parseOptions()
let started = Date()
var result = Result(source: opts.imagePath == nil ? "window" : "image")

let image: CGImage?
if let path = opts.imagePath {
    image = loadImage(path: path)
} else {
    image = await captureFrontmostWindow(opts: opts, result: &result)
}

if let image {
    result.width = image.width
    result.height = image.height
    do {
        let lines = try recognizeText(in: image, fast: opts.fast, correction: opts.correction).map { line in
            let (text, n) = redact(line.text)
            result.redactions += n
            return Line(text: text, confidence: line.confidence, x: line.x, y: line.y, w: line.w, h: line.h)
        }
        result.lines = Array(lines.prefix(opts.maxLines))
        result.columns = columns(from: readable(lines))
        result.mainText = mainText(from: result.columns)
    } catch {
        fail(4, "ocr_failed", error.localizedDescription)
    }
}

result.elapsedMs = Int(Date().timeIntervalSince(started) * 1000)
printJSON(result)
