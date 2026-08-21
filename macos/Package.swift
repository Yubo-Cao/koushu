// swift-tools-version: 6.0
import Foundation
import PackageDescription

// The Rust core is optional at build time, and deliberately so.
//
// `scripts/gen-swift-bindings.sh` writes generated Swift into `core/generated/`,
// which is gitignored — checked-in generated code drifts from what produced it.
// A fresh clone therefore has no bindings, and a package that required them
// would not build at all. Instead the app is written against protocols in
// `KoushuCore` and the Rust implementations are a target that only exists when
// the bindings do, selected here rather than by a flag somebody has to remember.
//
// `macos/build.sh` stages the generated files into `Sources/KoushuCoreFFI` and
// `Sources/KoushuRustCore` before calling swift build. They are staged rather
// than referenced in place because SPM refuses target paths outside the package
// directory, and `core/generated/` is one level up.
let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let repoRoot = packageDir.deletingLastPathComponent()

func exists(_ path: String) -> Bool {
    FileManager.default.fileExists(atPath: path)
}

let stagedBindings = packageDir.appendingPathComponent("Sources/KoushuRustCore/koushu_core.swift").path
let stagedHeader = packageDir.appendingPathComponent("Sources/KoushuCoreFFI/include/koushu_coreFFI.h").path
// Also staged, because cargo's output directory is not a path this file can
// derive: a workspace moves it to the workspace root, and a global `target-dir`
// in .cargo/config.toml can move it off the tree entirely.
let rustLibDir = packageDir.appendingPathComponent(".rustlib").path
let rustStaticLib = rustLibDir + "/libkoushu_core.a"

let hasRustCore = exists(stagedBindings) && exists(stagedHeader) && exists(rustStaticLib)

var targets: [Target] = [
    .target(
        name: "KoushuCore",
        path: "Sources/KoushuCore"
    ),
    .executableTarget(
        name: "Koushu",
        dependencies: ["KoushuCore"] + (hasRustCore ? ["KoushuRustCore"] : []),
        path: "Sources/Koushu",
        swiftSettings: hasRustCore ? [.define("KOUSHU_HAS_RUST_CORE")] : []
    ),
    .testTarget(
        name: "KoushuCoreTests",
        dependencies: ["KoushuCore"],
        path: "Tests/KoushuCoreTests"
    ),
]

if hasRustCore {
    targets.append(
        .systemLibrary(
            name: "KoushuCoreFFI",
            path: "Sources/KoushuCoreFFI"
        )
    )
    targets.append(
        .target(
            name: "KoushuRustCore",
            dependencies: ["KoushuCore", "KoushuCoreFFI"],
            path: "Sources/KoushuRustCore",
            linkerSettings: [
                // The staticlib rather than the dylib: an .app that loads a
                // dylib out of the repository's `target/` directory works on
                // this machine and nowhere else, and the failure would only
                // appear once it was copied somewhere.
                .unsafeFlags(["-L\(rustLibDir)", "-lkoushu_core"])
            ]
        )
    )
}

let package = Package(
    name: "Koushu",
    platforms: [.macOS("26.0")],
    targets: targets
)
