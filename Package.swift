// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "BTCTicker",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "TickerCore"),
        .executableTarget(name: "BTCTicker", dependencies: ["TickerCore"]),
        .testTarget(
            name: "TickerCoreTests",
            dependencies: ["TickerCore"],
            resources: [.copy("Fixtures")]
        ),
    ]
)
