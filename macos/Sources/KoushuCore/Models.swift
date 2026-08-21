import Foundation

// The domain, as the app talks about it.
//
// These mirror `lib/types.ts`, which mirrors the Rust structs, and they are
// written by hand on purpose: the ones that come from the extracted core will
// be replaced by UniFFI-generated types as each slice lands, and a hand-written
// stand-in that is later deleted is cheaper than a second source of truth that
// has to be kept in step forever.
//
// Where the TypeScript carries ISO-8601 strings, these carry `Date`. A string
// is what survives a JSON boundary; it is not what a view wants to format, and
// leaving the parse to each call site is how two parts of a UI end up
// disagreeing about what time something happened.

// MARK: - Sessions

/// A run of dictation. Sessions group transcripts and are what the sidebar
/// lists.
public struct SessionInfo: Identifiable, Hashable, Sendable {
    public var id: String
    public var title: String
    public var startedAt: Date
    public var endedAt: Date?
    /// `YYYY-MM-DD`, assigned by the core. Kept as a string rather than derived
    /// from `startedAt` here: which day a session belongs to is a storage
    /// decision (it is what the list groups by and what the date filters
    /// compare against), and recomputing it in the UI would put a second,
    /// timezone-dependent answer next to the authoritative one.
    public var dateKey: String
    public var model: String
    public var language: String
    public var runtime: String
    /// When the session was put away. Archiving hides; it never deletes.
    public var archivedAt: Date?

    public var isArchived: Bool { archivedAt != nil }

    public init(
        id: String,
        title: String,
        startedAt: Date,
        endedAt: Date? = nil,
        dateKey: String,
        model: String,
        language: String,
        runtime: String,
        archivedAt: Date? = nil
    ) {
        self.id = id
        self.title = title
        self.startedAt = startedAt
        self.endedAt = endedAt
        self.dateKey = dateKey
        self.model = model
        self.language = language
        self.runtime = runtime
        self.archivedAt = archivedAt
    }
}

/// Which side of the archive line to show.
public enum ArchiveScope: String, CaseIterable, Sendable {
    case active
    case archived
    case all
}

/// Narrowing shared by the session list and by search.
///
/// Every field is optional and `nil` means "no constraint", which is what a
/// picker set back to its "Any" option hands over. The same value narrows both
/// the list and the search deliberately: hits from sessions the sidebar is
/// hiding are hits the user cannot then open.
public struct SessionFilter: Hashable, Sendable {
    public var language: String?
    public var model: String?
    /// Inclusive `YYYY-MM-DD` bounds.
    public var from: String?
    public var to: String?
    public var archived: ArchiveScope

    public init(
        language: String? = nil,
        model: String? = nil,
        from: String? = nil,
        to: String? = nil,
        archived: ArchiveScope = .active
    ) {
        self.language = language
        self.model = model
        self.from = from
        self.to = to
        self.archived = archived
    }

    public static let none = SessionFilter()

    /// How many constraints are narrowing the view.
    ///
    /// Drives the count on the collapsed filter disclosure. `.active` is not
    /// counted because it is the default view, not a filter someone set.
    public var activeCount: Int {
        var count = 0
        if language?.isEmpty == false { count += 1 }
        if model?.isEmpty == false { count += 1 }
        if from?.isEmpty == false { count += 1 }
        if to?.isEmpty == false { count += 1 }
        if archived != .active { count += 1 }
        return count
    }

    public var isEmpty: Bool { activeCount == 0 }
}

/// The languages, models and dates that actually occur in the database.
///
/// The filter pickers are populated from this rather than from a fixed list, so
/// they can never offer a value that matches nothing.
public struct FilterOptions: Hashable, Sendable {
    public var languages: [String]
    public var models: [String]
    public var earliestDate: String?
    public var latestDate: String?
    public var archivedCount: Int

    public init(
        languages: [String] = [],
        models: [String] = [],
        earliestDate: String? = nil,
        latestDate: String? = nil,
        archivedCount: Int = 0
    ) {
        self.languages = languages
        self.models = models
        self.earliestDate = earliestDate
        self.latestDate = latestDate
        self.archivedCount = archivedCount
    }

    public static let empty = FilterOptions()
}

// MARK: - Transcripts

public struct TranscriptInfo: Identifiable, Hashable, Sendable {
    public var id: String
    public var sessionID: String
    /// What was said. Never overwritten — formatting is stored beside it.
    public var text: String
    public var status: String
    public var source: String
    public var createdAt: Date
    public var durationMS: Int?
    public var model: String
    public var language: String
    /// LLM-typeset Markdown.
    public var formattedText: String?
    public var formattedPreset: String?
    public var formattedAt: Date?

    public init(
        id: String,
        sessionID: String,
        text: String,
        status: String = "final",
        source: String = "local",
        createdAt: Date,
        durationMS: Int? = nil,
        model: String,
        language: String,
        formattedText: String? = nil,
        formattedPreset: String? = nil,
        formattedAt: Date? = nil
    ) {
        self.id = id
        self.sessionID = sessionID
        self.text = text
        self.status = status
        self.source = source
        self.createdAt = createdAt
        self.durationMS = durationMS
        self.model = model
        self.language = language
        self.formattedText = formattedText
        self.formattedPreset = formattedPreset
        self.formattedAt = formattedAt
    }
}

// MARK: - Search

public struct SearchHit: Identifiable, Hashable, Sendable {
    public var transcriptID: String
    public var sessionID: String
    public var sessionTitle: String
    public var dateKey: String
    public var createdAt: Date
    public var language: String
    public var model: String
    public var archived: Bool
    /// A window of the transcript around the first match, elided with `…`.
    public var snippet: String

    public var id: String { transcriptID }

    public init(
        transcriptID: String,
        sessionID: String,
        sessionTitle: String,
        dateKey: String,
        createdAt: Date,
        language: String,
        model: String,
        archived: Bool,
        snippet: String
    ) {
        self.transcriptID = transcriptID
        self.sessionID = sessionID
        self.sessionTitle = sessionTitle
        self.dateKey = dateKey
        self.createdAt = createdAt
        self.language = language
        self.model = model
        self.archived = archived
        self.snippet = snippet
    }
}

/// `substring` means a term was shorter than the three characters the trigram
/// index needs, so the query was answered by scanning instead. The distinction
/// matters for explaining an empty result, not for how hits are displayed.
public enum SearchMode: String, Sendable {
    case empty
    case fts
    case substring
}

public struct SearchResponse: Hashable, Sendable {
    /// The terms actually searched for, for highlighting inside each snippet.
    public var terms: [String]
    public var hits: [SearchHit]
    /// More matched than the limit; these are the most recent.
    public var truncated: Bool
    public var mode: SearchMode

    public init(terms: [String] = [], hits: [SearchHit] = [], truncated: Bool = false, mode: SearchMode = .empty) {
        self.terms = terms
        self.hits = hits
        self.truncated = truncated
        self.mode = mode
    }

    public static let empty = SearchResponse()
}

// MARK: - Models

public enum ModelStatus: String, Sendable {
    case available
    case downloading
    case installed
    case paused
    case error
}

public struct ModelInfo: Identifiable, Hashable, Sendable {
    public var id: String
    public var name: String
    /// One of the `BACKEND_*` identifiers in `ASRBackend`.
    public var backend: String
    public var source: String
    public var repoID: String
    public var localPath: String
    public var status: ModelStatus
    public var sizeBytes: Int64?
    public var installedAt: Date?
    public var lastError: String?

    public init(
        id: String,
        name: String,
        backend: String,
        source: String = "huggingface",
        repoID: String,
        localPath: String,
        status: ModelStatus = .available,
        sizeBytes: Int64? = nil,
        installedAt: Date? = nil,
        lastError: String? = nil
    ) {
        self.id = id
        self.name = name
        self.backend = backend
        self.source = source
        self.repoID = repoID
        self.localPath = localPath
        self.status = status
        self.sizeBytes = sizeBytes
        self.installedAt = installedAt
        self.lastError = lastError
    }
}

/// ASR backend identifiers.
///
/// These must stay in step with the `BACKEND_*` constants in the Rust side;
/// they are also the values stored in the `models.backend` column and the
/// `defaults.runtime` setting, so they are data, not display strings.
public enum ASRBackend {
    /// Fun-ASR-Nano: SAN-M encoder + Qwen3-0.6B decoder. Slower, more accurate.
    public static let nano = "funasr-nano-gguf-cpu"
    /// SenseVoiceSmall: encoder + CTC, one forward pass. Faster, weaker English.
    public static let senseVoice = "funasr-sensevoice-gguf-cpu"
    /// Fallback when no model is selected yet.
    public static let `default` = nano
}

public struct ModelDownloadState: Hashable, Sendable {
    public var modelID: String
    public var active: Bool
    public var paused: Bool
    public var downloadedBytes: Int64
    public var totalBytes: Int64?
    public var message: String

    public init(
        modelID: String,
        active: Bool = false,
        paused: Bool = false,
        downloadedBytes: Int64 = 0,
        totalBytes: Int64? = nil,
        message: String = ""
    ) {
        self.modelID = modelID
        self.active = active
        self.paused = paused
        self.downloadedBytes = downloadedBytes
        self.totalBytes = totalBytes
        self.message = message
    }
}

/// Progress from a download in flight.
///
/// A stream, not a poll: the cadence is the core's to decide, and a UI that
/// asks "how far along is it?" on a timer of its own invention is a UI that
/// will disagree with the other platform about what "downloading" looks like.
public enum ModelDownloadEvent: Sendable {
    case started(modelID: String, downloadedBytes: Int64, totalBytes: Int64?)
    case progress(modelID: String, downloadedBytes: Int64, totalBytes: Int64?)
    case paused(modelID: String, downloadedBytes: Int64, totalBytes: Int64?)
    case finished(modelID: String, model: ModelInfo)
    case failed(modelID: String, message: String)
}

// MARK: - Audio

public struct AudioInputInfo: Identifiable, Hashable, Sendable {
    public var id: String
    public var name: String
    public var isDefault: Bool

    public init(id: String, name: String, isDefault: Bool = false) {
        self.id = id
        self.name = name
        self.isDefault = isDefault
    }
}

// MARK: - Transcription

/// What comes back from a decode.
///
/// `partial` is a preview and is reserved: the VAD-segmented streaming worker
/// that produces them is still inside `src-tauri` and has not been extracted, so
/// nothing emits it yet. It is declared because leaving it out would shape the
/// interface around one runtime's current limitation, and the whole point of the
/// core is that the shells do not have to be rewritten when it gains one.
///
/// `committed` is the authoritative text: the whole recording, decoded in one
/// pass. That is deliberately not the same as joining the partials — segment
/// boundaries cut mid-sentence and short spans decode worse.
public enum TranscriptionEvent: Sendable {
    /// The current, still-changing decode of the segment in progress.
    case partial(segmentIndex: Int, text: String)
    /// The whole utterance. `elapsedMS` is how long the runtime took, not how
    /// long the audio was — it is what answers "is this fast enough on this
    /// machine?", which is the question that decides which model to use.
    case committed(text: String, elapsedMS: Int)
    /// Actionable text from the core. Show it; do not re-translate it.
    case failed(String)
    /// The runtime ran and heard nothing worth transcribing. Not a failure, and
    /// deliberately distinct from an empty `committed`: one means "you were
    /// silent", the other means "the decoder returned nothing", and they send
    /// the user to look at different things.
    case noSpeech
}

/// The languages offered for transcription.
///
/// Endonyms, and never translated: they are what the user would call the
/// language they are about to speak, and they are stored in the database as
/// spoken-language ids rather than as display strings.
public let transcriptionLanguages = [
    "中文",
    "English",
    "日本語",
    "粤语",
    "한국어",
    "Français",
    "Deutsch",
    "Español",
]

// MARK: - LLM formatting

public struct FormatPreset: Identifiable, Hashable, Sendable {
    public var id: String
    public var label: String
    public var description: String
    public var prompt: String

    public init(id: String, label: String, description: String, prompt: String = "") {
        self.id = id
        self.label = label
        self.description = description
        self.prompt = prompt
    }
}

public struct LLMSettings: Hashable, Sendable {
    public var baseURL: String
    public var model: String
    /// Whether a key is stored. The key itself never crosses this boundary.
    public var hasAPIKey: Bool
    public var preset: String
    public var autoFormat: Bool
    public var presets: [FormatPreset]

    public init(
        baseURL: String = "",
        model: String = "",
        hasAPIKey: Bool = false,
        preset: String = "typeset",
        autoFormat: Bool = false,
        presets: [FormatPreset] = []
    ) {
        self.baseURL = baseURL
        self.model = model
        self.hasAPIKey = hasAPIKey
        self.preset = preset
        self.autoFormat = autoFormat
        self.presets = presets
    }

    /// Formatting is only offered once there is somewhere to send it.
    public var isConfigured: Bool { !baseURL.trimmingCharacters(in: .whitespaces).isEmpty }
}

public enum FormatEvent: Sendable {
    case delta(String)
    case done(String)
    case failed(String)
}

// MARK: - Licence and trial

public struct LicenseInfo: Hashable, Sendable {
    public var valid: Bool
    public var email: String?
    public var issued: String?
    /// Why a licence was rejected, in words the user can act on.
    public var detail: String

    public init(valid: Bool, email: String? = nil, issued: String? = nil, detail: String) {
        self.valid = valid
        self.email = email
        self.issued = issued
        self.detail = detail
    }
}

/// Trial metering. Counts VAD-detected speech, not how long the key was held.
public struct TrialStatus: Hashable, Sendable {
    public var usedSeconds: Double
    public var limitSeconds: Double
    public var licensed: Bool
    /// Set once, on the first transcript ever produced.
    public var firstTranscript: Bool

    public init(
        usedSeconds: Double = 0,
        limitSeconds: Double = 1800,
        licensed: Bool = false,
        firstTranscript: Bool = false
    ) {
        self.usedSeconds = usedSeconds
        self.limitSeconds = limitSeconds
        self.licensed = licensed
        self.firstTranscript = firstTranscript
    }

    public var fraction: Double {
        guard limitSeconds > 0 else { return 0 }
        return min(1, usedSeconds / limitSeconds)
    }
}

// MARK: - Platform

/// What the host can do, for the diagnostics panel.
///
/// Narrower than the Tauri version's `PlatformInfo`: the Wayland/X11/paste-tool
/// fields exist to explain why pasting might not work on Linux, and on macOS
/// there is exactly one answer, so reporting the fields would be reporting
/// constants.
public struct PlatformInfo: Hashable, Sendable {
    public var os: String
    public var arch: String
    public var bundledASR: Bool

    public init(os: String, arch: String, bundledASR: Bool) {
        self.os = os
        self.arch = arch
        self.bundledASR = bundledASR
    }
}

// MARK: - Settings keys

/// The rows of the settings table this app reads or writes.
///
/// Spelled once, here, because they are a wire format shared with the Tauri
/// build: the same database is read by both, so a typo is not a compile error
/// on either side, it is a setting that silently stops being honoured.
public enum SettingKey {
    public static let defaultModel = "defaults.model"
    public static let defaultLanguage = "defaults.language"
    public static let defaultRuntime = "defaults.runtime"
    public static let uiLocale = "ui.locale"
    public static let retainAudio = "audio.retain"
    public static let autoPaste = "floating.autoPaste"
    public static let liveInsert = "floating.liveInsert"
    public static let audioInput = "audio.input"
    public static let pushToTalkTrigger = "hotkey.pushToTalk"
    public static let cloudBaseURL = "asr.cloud.baseUrl"
    public static let cloudModel = "asr.cloud.model"
    public static let cloudLanguage = "asr.cloud.language"
    public static let llmBaseURL = "llm.baseUrl"
    public static let llmModel = "llm.model"
    public static let llmPreset = "llm.preset"
}
