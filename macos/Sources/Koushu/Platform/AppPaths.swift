import Foundation

/// Where the app's files are.
///
/// The data directory is **shared with the Tauri build**, deliberately. It is
/// the same product: the same models (911 MB of Fun-ASR-Nano that nobody should
/// download twice), the same session history, the same settings rows. Giving the
/// native app its own directory would have meant a second copy of everything and
/// a history that silently forked the day it was installed.
///
/// The directory is named after the Tauri bundle id because that is what is
/// already on disk. Renaming it would be a migration, and a migration is a thing
/// to do deliberately once the native app is the one people use — not as a side
/// effect of it starting to work.
enum AppPaths {
    static let sharedDataDirectoryName = "dev.yubo.fun-asr-desktop"

    static var dataDirectory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent(sharedDataDirectoryName, isDirectory: true)
    }

    static var database: URL {
        dataDirectory.appendingPathComponent("fun_asr_desktop.sqlite3")
    }

    static func modelDirectory(_ modelID: String) -> URL {
        dataDirectory.appendingPathComponent("models", isDirectory: true)
            .appendingPathComponent(modelID, isDirectory: true)
    }

    /// Recordings in flight. Separate from `audio/`, which is where the Tauri
    /// build keeps ones the user asked to retain — a temporary file that lands
    /// in the same place as a kept one is a temporary file somebody will
    /// eventually treat as data.
    static var scratchAudioDirectory: URL {
        let directory = dataDirectory
            .appendingPathComponent("audio", isDirectory: true)
            .appendingPathComponent("native-incoming", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    // MARK: Bundled runtimes

    /// The official llama.cpp runtime binaries, inside the app bundle.
    ///
    /// `Contents/Resources/binaries`, matching where the Tauri bundle puts them,
    /// so the two builds can be compared without also arguing about layout. In a
    /// `swift run` build there is no bundle, so the repository copy is the
    /// fallback — which keeps the thing runnable from a terminal without
    /// assembling an `.app` first.
    static var runtimeDirectory: URL {
        if let resources = Bundle.main.resourceURL {
            let bundled = resources.appendingPathComponent("binaries", isDirectory: true)
            if FileManager.default.fileExists(atPath: bundled.path) { return bundled }
        }
        return URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()   // Platform
            .deletingLastPathComponent()   // Koushu
            .deletingLastPathComponent()   // Sources
            .deletingLastPathComponent()   // macos
            .appendingPathComponent("src-tauri/binaries", isDirectory: true)
    }

    /// The binaries are named with their target triple, because one repository
    /// holds every platform's copy and a bare `llama-funasr-cli` would be
    /// whichever one was extracted last.
    static var nanoCLI: URL { runtimeDirectory.appendingPathComponent("llama-funasr-cli-aarch64-apple-darwin") }
    static var senseVoiceCLI: URL { runtimeDirectory.appendingPathComponent("llama-funasr-sensevoice-aarch64-apple-darwin") }

    static var runtimesPresent: Bool {
        let manager = FileManager.default
        return manager.isExecutableFile(atPath: nanoCLI.path)
            && manager.isExecutableFile(atPath: senseVoiceCLI.path)
    }
}
