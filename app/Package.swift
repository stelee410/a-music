// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "AgentGroovebox",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "AgentGroovebox",
            path: "Sources/AgentGroovebox"
        )
    ]
)
