import Foundation

// The seam to the shared Rust core.
//
// `core/` currently exports exactly one thing across UniFFI — licence
// verification — because slice 1 moved the three modules with no Tauri
// dependency and stopped there. Storage, the ASR runtime, model management and
// the LLM client are still inside `src-tauri/src/lib.rs`, and until they move
// there is nothing for Swift to call.
//
// So they are declared here as protocols and implemented by `StubCore`. That is
// not a placeholder for its own sake: writing the protocols first is what forces
// the boundary to be the shape `docs/core-extraction.md` argues for, before
// there is any code to be shaped by instead —
//
//   * everything long-running is a **stream**. Transcription emits partials
//     before it commits, downloads emit progress, formatting streams tokens. A
//     `func progress() -> Int` would put the cadence policy in the UI, which is
//     exactly where the two platforms would start to disagree;
//   * every stream is **cancellable**, because push-to-talk is released
//     mid-utterance constantly and "drop the task" does not cross a C ABI;
//   * failures carry **actionable text**, not codes. The core already returns
//     sentences a user can act on; re-deriving them per platform would mean
//     writing them twice and translating them twice.
//
// When a slice lands, the corresponding stub is replaced by a UniFFI-backed
// implementation and nothing above this file changes.

// MARK: - Cancellation

/// A handle to work in flight.
///
/// Deliberately not `Task`: the work being cancelled lives on the far side of an
/// FFI boundary, so cancellation has to be a call it can observe rather than a
/// cooperative check on this side.
public protocol CoreCancellable: Sendable {
    func cancel()
}

// MARK: - Sessions and transcripts

public protocol SessionStore: Sendable {
    /// Newest first. The filter is applied by the store rather than by the
    /// caller: a date range or an archive scope has to be evaluated across the
    /// whole history, not across whichever page of it happens to be loaded.
    func sessions(limit: Int, filter: SessionFilter) async throws -> [SessionInfo]

    func createSession(title: String, model: String, language: String, runtime: String) async throws -> SessionInfo

    func transcripts(sessionID: String) async throws -> [TranscriptInfo]

    /// Puts a session away, or brings it back. Nothing is deleted either way.
    @discardableResult
    func setArchived(sessionID: String, archived: Bool) async throws -> SessionInfo?

    /// The values that actually occur, for populating the filter pickers.
    func filterOptions() async throws -> FilterOptions

    /// Full-text search across every transcript, newest match first.
    ///
    /// Fast enough to run on every keystroke — it is a local index — which is
    /// why there is no notion of a search being "started".
    func search(query: String, filter: SessionFilter, limit: Int) async throws -> SearchResponse

    /// Store what was just said.
    ///
    /// Returns the stored row rather than void: the id and the timestamp are the
    /// store's to assign, and a caller that made up its own would be showing the
    /// user something the database does not contain.
    @discardableResult
    func appendTranscript(
        sessionID: String,
        text: String,
        model: String,
        language: String,
        durationMS: Int?
    ) async throws -> TranscriptInfo

    /// Store the Markdown a formatting pass produced, beside the spoken text.
    @discardableResult
    func saveFormatted(transcriptID: String, markdown: String, preset: String) async throws -> TranscriptInfo?
}

// MARK: - Settings

public protocol SettingsStore: Sendable {
    func all() async throws -> [String: String]
    func value(for key: String) async throws -> String?
    func set(_ key: String, to value: String) async throws
}

extension SettingsStore {
    public func string(for key: String, default fallback: String) async -> String {
        let stored = try? await value(for: key)
        guard let stored, !stored.isEmpty else { return fallback }
        return stored
    }

    public func flag(for key: String, default fallback: Bool) async -> Bool {
        guard let stored = try? await value(for: key) else { return fallback }
        switch stored {
        case "true": return true
        case "false": return false
        default: return fallback
        }
    }
}

// MARK: - Models

public protocol ModelCatalog: Sendable {
    func models() async throws -> [ModelInfo]

    /// Start or resume a download. The returned handle cancels it; pausing is a
    /// separate verb because a paused download keeps its bytes and a cancelled
    /// one need not.
    func download(modelID: String, onEvent: @escaping @Sendable (ModelDownloadEvent) -> Void) -> CoreCancellable

    func pauseDownload(modelID: String) async
}

// MARK: - Transcription

/// A finished recording, waiting to be decoded.
///
/// A file path rather than a buffer, and this is the boundary on purpose. The
/// two shells capture audio in completely different ways — `cpal` on Linux,
/// `AVAudioEngine` on macOS — and neither is better in the abstract, so capture
/// stays with the platform. The runtimes take a path anyway, so handing over
/// bytes would mean copying tens of megabytes across a C ABI in order to write
/// them straight back out to a temporary file.
public struct TranscriptionRequest: Sendable {
    public var wavPath: String
    public var modelID: String
    /// One of the `ASRBackend` ids. Decides which runtime binary runs.
    public var backend: String
    /// The spoken language the user selected. Fun-ASR-Nano detects language
    /// itself and ignores this; it is still carried because it is what gets
    /// stored on the transcript, and because the cloud backend does use it.
    public var language: String

    public init(wavPath: String, modelID: String, backend: String, language: String) {
        self.wavPath = wavPath
        self.modelID = modelID
        self.backend = backend
        self.language = language
    }
}

public protocol TranscriptionEngine: Sendable {
    /// Decode a recording. Returns immediately; results arrive on `onEvent`.
    ///
    /// Cancellable because these runs take seconds — long enough that somebody
    /// who has changed their mind should not be waiting for a result nobody will
    /// read. Cancelling kills the runtime process; it is not a co-operative
    /// flag that gets checked at the end.
    func transcribe(
        _ request: TranscriptionRequest,
        onEvent: @escaping @Sendable (TranscriptionEvent) -> Void
    ) -> CoreCancellable

    /// Which model files this backend needs and cannot find. Empty means ready.
    ///
    /// Asked before recording rather than after, so a missing model is a
    /// sentence on screen instead of a recording that turns out to have been
    /// pointless.
    func missingAssets(modelID: String, backend: String) -> [String]
}

// MARK: - LLM formatting

public protocol LLMFormatter: Sendable {
    func settings() async throws -> LLMSettings

    /// `nil` clears the stored key. The key is write-only from the UI's side —
    /// it goes to the keychain and never comes back.
    func setAPIKey(_ key: String?) async throws

    func format(
        transcriptID: String?,
        text: String,
        preset: String?,
        onEvent: @escaping @Sendable (FormatEvent) -> Void
    ) -> CoreCancellable
}

// MARK: - Licence and trial

public protocol LicenseService: Sendable {
    /// Never throws for a rejected licence. A rejection is an answer with text
    /// in it, not a failure — modelling it as an error forces every caller to
    /// unwrap it back into the sentence it already was.
    func verify(_ license: String) -> LicenseInfo
}

public protocol TrialMeter: Sendable {
    func status() async throws -> TrialStatus
}

// MARK: - Audio devices

public protocol AudioDeviceSource: Sendable {
    func inputs() async -> [AudioInputInfo]
}

// MARK: - Assembly

/// Everything the app reaches the core through, in one value.
///
/// One injection point rather than eight, so swapping the stubs for the real
/// implementations as slices land is a change to one initialiser and nothing
/// else, and so a preview or a test can substitute a single object.
public struct CoreServices: Sendable {
    public var sessions: any SessionStore
    public var settings: any SettingsStore
    public var models: any ModelCatalog
    public var transcription: any TranscriptionEngine
    public var formatter: any LLMFormatter
    public var license: any LicenseService
    public var trial: any TrialMeter
    public var audioDevices: any AudioDeviceSource
    public var platform: PlatformInfo

    public init(
        sessions: any SessionStore,
        settings: any SettingsStore,
        models: any ModelCatalog,
        transcription: any TranscriptionEngine,
        formatter: any LLMFormatter,
        license: any LicenseService,
        trial: any TrialMeter,
        audioDevices: any AudioDeviceSource,
        platform: PlatformInfo
    ) {
        self.sessions = sessions
        self.settings = settings
        self.models = models
        self.transcription = transcription
        self.formatter = formatter
        self.license = license
        self.trial = trial
        self.audioDevices = audioDevices
        self.platform = platform
    }
}
