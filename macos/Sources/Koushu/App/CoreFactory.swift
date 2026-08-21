import Foundation
import KoushuCore

#if KOUSHU_HAS_RUST_CORE
import KoushuRustCore
#endif

/// Assembles the services the app talks to.
///
/// One function, so that swapping a stub for a real implementation as each slice
/// of `docs/core-extraction.md` lands is an edit here and nowhere else.
///
/// The licence service is the only one with a real implementation today, and it
/// is compiled in only when `core/generated/swift` exists — which is what makes
/// a fresh clone still build. That conditional is not a temporary scaffold: it
/// is how every subsequent slice will arrive, one `#if` at a time, with the app
/// continuing to run against stubs for everything that has not moved yet.
enum CoreFactory {
    @MainActor
    static func make() -> CoreServices {
        let database = StubDatabase()

        // One store serving three protocols, because it is one file: an archive
        // has to be visible to the next search and a settings write to the next
        // read.
        var sessions: any SessionStore = StubSessionStore(database: database)
        var settings: any SettingsStore = StubSettingsStore(database: database)
        var models: any ModelCatalog = StubModelCatalog(database: database)

        #if KOUSHU_HAS_RUST_CORE
        // Opening can fail — an unwritable directory, a corrupt file — and the
        // honest response is to fall back to the in-memory stubs rather than
        // refuse to launch. The app is still useful with no history, and a
        // menu-bar app that dies silently at startup is the worst outcome there
        // is: nothing appears, and nothing says why.
        do {
            let store = try RustStore(databasePath: AppPaths.database.path)
            store.pruneEmptySessions()
            try? store.seedModels(builtInModels)
            sessions = store
            settings = store
            models = store
        } catch {
            NSLog("[storage] falling back to memory: \(error)")
        }
        #endif

        return CoreServices(
            sessions: sessions,
            settings: settings,
            models: models,
            transcription: transcriptionEngine,
            formatter: StubLLMFormatter(database: database),
            license: licenseService,
            trial: StubTrialMeter(),
            audioDevices: SystemAudioDevices(),
            platform: PlatformInfo(
                os: "macOS \(ProcessInfo.processInfo.operatingSystemVersionString)",
                arch: architecture,
                // Checked rather than asserted. "The runtime is bundled" is a
                // claim about two files being inside this .app, and the Models
                // tab shows it as a status line, so it has to be the answer to
                // that question rather than a constant somebody set once.
                bundledASR: AppPaths.runtimesPresent
            )
        )
    }

    /// Real recognition when the core is linked, a placeholder otherwise.
    ///
    /// The model directory is injected rather than looked up inside the core:
    /// where models live is a question about this platform's directory layout,
    /// and the core has no business guessing at it. Here the answer is the
    /// **Tauri build's** data directory — see `AppPaths` for why sharing it is
    /// deliberate rather than an oversight.
    private static var transcriptionEngine: any TranscriptionEngine {
        #if KOUSHU_HAS_RUST_CORE
        RustTranscriptionEngine(
            nanoCLI: AppPaths.nanoCLI.path,
            senseVoiceCLI: AppPaths.senseVoiceCLI.path,
            modelDirectory: { AppPaths.modelDirectory($0).path }
        )
        #else
        StubTranscriptionEngine()
        #endif
    }

    /// The built-in catalogue, inserted only if the row is not already there.
    ///
    /// `localPath` points at the shared data directory, so a model the Tauri
    /// build downloaded is the same row this one uses — which is the whole point
    /// of sharing the directory.
    static let builtInModels: [ModelInfo] = [
        ModelInfo(
            id: "fun-asr-nano-2512",
            name: "Fun-ASR-Nano 2512 (GGUF Q4_K)",
            backend: ASRBackend.nano,
            repoID: "FunAudioLLM/Fun-ASR-Nano-GGUF",
            localPath: AppPaths.modelDirectory("fun-asr-nano-2512").path,
            status: .available
        ),
        ModelInfo(
            id: "sensevoice-small",
            name: "SenseVoiceSmall (GGUF Q8_0)",
            backend: ASRBackend.senseVoice,
            repoID: "FunAudioLLM/SenseVoiceSmall-GGUF",
            localPath: AppPaths.modelDirectory("sensevoice-small").path,
            status: .available
        ),
    ]

    private static var licenseService: any LicenseService {
        #if KOUSHU_HAS_RUST_CORE
        RustLicenseService()
        #else
        StubLicenseService()
        #endif
    }

    private static var architecture: String {
        #if arch(arm64)
        "arm64"
        #elseif arch(x86_64)
        "x86_64"
        #else
        "unknown"
        #endif
    }
}

/// The microphones macOS is offering, asked at the moment they are needed.
///
/// Not a stub: device enumeration is a platform question, not a core one, and it
/// is deliberately left on this side of the boundary for the same reason windows
/// and hotkeys are.
struct SystemAudioDevices: AudioDeviceSource {
    func inputs() async -> [AudioInputInfo] {
        await MainActor.run { AudioLevelMonitor.inputs() }
    }
}
