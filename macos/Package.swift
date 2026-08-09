// swift-tools-version: 6.0
import PackageDescription

// Prototype only. No Rust / FFI here on purpose: the shared core is being
// extracted on another branch and has no Swift package yet.
let package = Package(
    name: "FunASRBar",
    platforms: [.macOS("26.0")],
    targets: [
        .executableTarget(
            name: "FunASRBar",
            path: "Sources/FunASRBar",
            swiftSettings: [.swiftLanguageMode(.v5)]
        )
    ]
)
