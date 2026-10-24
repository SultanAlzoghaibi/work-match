// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "wm-ocr",
    platforms: [.macOS(.v14)],  // SCScreenshotManager needs macOS 14
    targets: [
        .executableTarget(
            name: "wm-ocr",
            path: "Sources/wm-ocr"
        ),
    ]
)
