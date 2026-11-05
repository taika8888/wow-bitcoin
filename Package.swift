// swift-tools-version:6.0
import PackageDescription
import Foundation

let rustLibrary = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("windows/target/release/libticker_ffi.a").path

let package = Package(
    name: "BTCTicker",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "CTickerCore", publicHeadersPath: "include"),
        .target(name: "TickerCore", dependencies: ["CTickerCore"], linkerSettings: [
            .unsafeFlags([rustLibrary]),
        ]),
        .executableTarget(name: "BTCTicker", dependencies: ["TickerCore"]),
        .testTarget(
            name: "TickerCoreTests",
            dependencies: ["TickerCore"],
            resources: [.copy("Fixtures")]
        ),
    ]
)
