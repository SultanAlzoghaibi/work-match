// End-to-end checks for wm-ocr: renders test screens, runs the binary on
// them, and checks the JSON. Run through ../test.sh.
//
//   swift tests/check.swift .build/release/wm-ocr

import AppKit

let binary = CommandLine.arguments.dropFirst().first ?? ".build/release/wm-ocr"
let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("wm-ocr-tests-\(getpid())")
try FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: tmp) }

var failures = 0

func check(_ ok: Bool, _ name: String) {
    print(ok ? "  ok    \(name)" : "  FAIL  \(name)")
    if !ok { failures += 1 }
}

/// Draws columns of monospaced text on a dark background, like an editor.
func render(_ name: String, width: CGFloat, height: CGFloat, columns: [(x: CGFloat, lines: [String])]) -> String {
    let size = NSSize(width: width, height: height)
    let image = NSImage(size: size)
    image.lockFocus()
    NSColor(calibratedWhite: 0.12, alpha: 1).setFill()
    NSRect(origin: .zero, size: size).fill()
    let attrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedSystemFont(ofSize: 22, weight: .regular),
        .foregroundColor: NSColor(calibratedWhite: 0.9, alpha: 1),
    ]
    for column in columns {
        for (i, line) in column.lines.enumerated() {
            (line as NSString).draw(at: NSPoint(x: column.x, y: height - 70 - CGFloat(i) * 44), withAttributes: attrs)
        }
    }
    image.unlockFocus()
    let path = tmp.appendingPathComponent(name).path
    let png = NSBitmapImageRep(data: image.tiffRepresentation!)!.representation(using: .png, properties: [:])!
    try! png.write(to: URL(fileURLWithPath: path))
    return path
}

/// Runs wm-ocr and returns (exit code, parsed JSON).
func run(_ args: [String]) -> (Int32, [String: Any]) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: binary)
    process.arguments = args
    let out = Pipe()
    process.standardOutput = out
    try! process.run()
    let data = out.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    return (process.terminationStatus, json)
}

func position(of needle: String, in text: String) -> Int? {
    text.range(of: needle).map { text.distance(from: text.startIndex, to: $0.lowerBound) }
}

// 1. Columns: sidebar, editor and chat panel must not interleave.
print("columns")
let columnsImage = render("columns.png", width: 1800, height: 1000, columns: [
    (30, ["EXPLORER", "backend", "docs", "DESIGN.md", "SPEC.md", "tauri"]),
    (330, ["import heapq", "", "def dijkstra(graph, src):", "    dist = {n: float('inf') for n in graph}",
           "    pq = [(0, src)]", "    while pq:", "        d, u = heapq.heappop(pq)", "    return dist"]),
    (1300, ["CHAT", "Why are distances wrong?", "Check the relax step"]),
])
do {
    let (code, r) = run(["--image", columnsImage, "--no-correction"])
    let text = r["main_text"] as? String ?? ""
    let cols = r["columns"] as? [[String: Any]] ?? []
    check(code == 0, "exits 0")
    check(cols.count == 3, "finds 3 columns (got \(cols.count))")
    check((cols.first?["text"] as? String)?.contains("def dijkstra") == true, "editor column comes first")
    if let code = position(of: "def dijkstra", in: text), let chat = position(of: "CHAT", in: text),
       let tree = position(of: "EXPLORER", in: text), let ret = position(of: "return dist", in: text) {
        check(code < ret && ret < chat && ret < tree, "editor lines stay together, before the side panels")
    } else {
        check(false, "all three columns are present in main_text")
    }
}

// 2. Secrets: every fake secret is replaced, normal code and URLs are kept.
print("secrets")
let fakes = [
    "DATABASE_URL=postgresql://postgres.abc:FakePass123@db.example.com:5432/postgres",
    "OPENROUTER_API_KEY=sk-or-v1-FAKEFAKEFAKE1234567890abcdef",
    "password: hunter2fake",
    "export AWS_KEY=AKIAFAKEFAKEFAKE1234",
    "token = ghp_FAKEfakeFAKEfake1234567890",
    "-----BEGIN RSA PRIVATE KEY-----",
]
let secretsImage = render("secrets.png", width: 1600, height: 560, columns: [
    (30, fakes + ["def dijkstra(graph, src):", "Solving LeetCode 743 Network Delay Time",
                  "see https://leetcode.com/problems/network-delay-time/"]),
])
do {
    let (code, r) = run(["--image", secretsImage, "--no-correction"])
    let everything = String(data: try JSONSerialization.data(withJSONObject: r), encoding: .utf8) ?? ""
    check(code == 0, "exits 0")
    check((r["redactions"] as? Int ?? 0) >= fakes.count, "counts at least \(fakes.count) redactions (got \(r["redactions"] ?? "none"))")
    for secret in ["FakePass123", "FAKEFAKEFAKE", "hunter2fake", "AKIAFAKE", "ghp_FAKE", "BEGIN RSA"] {
        check(!everything.contains(secret), "hides \(secret)")
    }
    let text = r["main_text"] as? String ?? ""
    check(text.contains("def dijkstra"), "keeps normal code")
    check(text.contains("leetcode.com/problems/network-delay-time"), "keeps normal URLs")
    check(text.contains("db.example.com:5432"), "keeps the host of a redacted URL")
}

// 3. Errors are JSON with a non-zero exit code.
print("errors")
do {
    let (code, r) = run(["--bogus"])
    check(code == 1 && r["error"] as? String == "bad_args", "unknown flag -> bad_args, exit 1")
    let (code2, r2) = run(["--image", tmp.appendingPathComponent("missing.png").path])
    check(code2 == 1 && r2["error"] as? String == "bad_image", "missing image -> bad_image, exit 1")
}

print(failures == 0 ? "\nall checks passed" : "\n\(failures) check(s) failed")
exit(failures == 0 ? 0 : 1)
