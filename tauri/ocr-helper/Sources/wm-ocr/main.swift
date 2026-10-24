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
    var mainText: String = ""    // lines in reading order, chrome stripped
    var skipped: Bool = false
    var elapsedMs: Int = 0

    enum CodingKeys: String, CodingKey {
        case app, source, width, height, lines, skipped
        case bundleId = "bundle_id"
        case windowTitle = "window_title"
        case mainText = "main_text"
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
        return nil
    }

    do {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        guard let window = frontmostWindow(content: content, pid: front.processIdentifier) else {
            fail(3, "no_window", "\(front.localizedName ?? "App") has no visible window")
        }
        result.windowTitle = window.title

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

/// Reading order (top to bottom, then left to right), minus low-confidence
/// noise and the thin bands where title bars and status bars sit.
func mainText(from lines: [Line]) -> String {
    lines
        .filter { $0.confidence >= 0.5 && $0.y > 0.03 && $0.y + $0.h < 0.97 }
        .sorted { abs($0.y - $1.y) > 0.01 ? $0.y < $1.y : $0.x < $1.x }
        .map(\.text)
        .joined(separator: "\n")
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
        let lines = try recognizeText(in: image, fast: opts.fast, correction: opts.correction)
        result.lines = Array(lines.prefix(opts.maxLines))
        result.mainText = mainText(from: lines)
    } catch {
        fail(4, "ocr_failed", error.localizedDescription)
    }
}

result.elapsedMs = Int(Date().timeIntervalSince(started) * 1000)
printJSON(result)
