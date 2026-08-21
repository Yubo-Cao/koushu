import AppKit
import Foundation
import KoushuCore
import Observation
import SwiftUI

/// What the app is doing, in the only terms the user can see it in.
///
/// The menu-bar icon, the voice bar and the main window all read from this one
/// value rather than each keeping their own flag. The Tauri build learned this
/// the hard way: the tray polls `AppState` for a recording flag and a job count
/// precisely so there is one answer, and two surfaces disagreeing about whether
/// the microphone is open is the worst thing this app can do.
public enum Activity: String, Sendable {
    case idle
    case recording
    /// The key is up and the whole recording is being decoded in one pass. Not
    /// a spinner state — it is the part of the interaction where the user is
    /// waiting for words, and hiding it makes the app look stuck.
    case transcribing
}

/// The one place the interface's state lives.
///
/// `@Observable` and `@MainActor`: every mutation here ends up on screen, and
/// the alternative — a value type threaded through bindings — would mean the
/// voice bar, the menu bar item and two windows each holding a copy of the
/// recording flag.
@MainActor
@Observable
public final class AppModel {
    // MARK: Services

    public let core: CoreServices

    // MARK: Language

    public private(set) var locale: UILocale
    /// `l(.deckTalk)` at the call site.
    public var l: Localizer { Localizer(locale: locale) }

    // MARK: Activity

    public private(set) var activity: Activity = .idle
    /// The live decode, replaced by the committed transcript when it lands.
    /// Never stored: it is a preview, and segment boundaries cut mid-sentence.
    public var partial: String = ""
    /// What the last committed utterance said, for the voice bar to show.
    public var lastTranscript: String = ""
    public var elapsed: TimeInterval = 0

    /// Newest sample last. Drives the meter in both the bar and the deck.
    public var levels: [CGFloat] = Array(repeating: 0, count: AppModel.levelCount)
    public var level: CGFloat = 0
    public static let levelCount = 30

    // MARK: Status line

    /// One sentence, in the title bar. Empty means "nothing has happened yet",
    /// which renders as the idle label in whatever locale is current — storing
    /// the translated string would freeze it at the locale it was set in.
    public var status: Msg?

    /// Text that came from the core rather than from us. Shown verbatim: the
    /// core's failures already carry a sentence the user can act on, and
    /// re-translating them would mean writing every one of them twice.
    public var coreMessage: String = ""

    // MARK: Settings mirror

    public var defaultModelID: String = "fun-asr-nano-2512"
    public var defaultLanguage: String = "中文"
    public var audioInputID: String = ""
    public var retainAudio = false
    public var autoInsert = true
    public var liveInsert = false
    public var chord: Chord = .default

    public var models: [ModelInfo] = []
    public var audioInputs: [AudioInputInfo] = []
    public var trial: TrialStatus?
    public var llm = LLMSettings()

    // MARK: Permissions

    /// Whether the microphone has been granted. `nil` until it has been asked
    /// about — which is not on launch, because asking puts a system dialog on
    /// screen and the app is meant to be inert until it is used.
    public var microphoneGranted: Bool?
    /// Whether the process is trusted for Accessibility. Cached: `AXIsProcessTrusted`
    /// is a synchronous IPC to tccd, and polling it fills the system log.
    public var accessibilityTrusted = false
    /// Whether a listener is actually running on `chord`.
    public var hotkeyArmed = false

    public var selectedModel: ModelInfo? {
        models.first { $0.id == defaultModelID }
    }

    public var runtime: String {
        selectedModel?.backend ?? ASRBackend.default
    }

    public init(core: CoreServices, locale: UILocale = .system) {
        self.core = core
        self.locale = locale
    }

    // MARK: Loading

    /// Read everything the windows need, once.
    ///
    /// Called when a window is first opened rather than at launch: an accessory
    /// process that has shown nothing has no reason to have touched the
    /// database, and doing it lazily keeps a bare launch genuinely inert.
    public func load() async {
        let stored = (try? await core.settings.all()) ?? [:]
        if let raw = stored[SettingKey.uiLocale], let saved = UILocale(rawValue: raw) {
            locale = saved
        }
        defaultModelID = stored[SettingKey.defaultModel] ?? defaultModelID
        defaultLanguage = stored[SettingKey.defaultLanguage] ?? defaultLanguage
        audioInputID = stored[SettingKey.audioInput] ?? ""
        retainAudio = stored[SettingKey.retainAudio] == "true"
        autoInsert = stored[SettingKey.autoPaste] != "false"
        liveInsert = stored[SettingKey.liveInsert] == "true"
        if let raw = stored[SettingKey.pushToTalkTrigger], let saved = Chord(stored: raw) {
            chord = saved
        }

        models = (try? await core.models.models()) ?? []
        audioInputs = await core.audioDevices.inputs()
        trial = try? await core.trial.status()
        llm = (try? await core.formatter.settings()) ?? LLMSettings()
    }

    public func refreshModels() async {
        models = (try? await core.models.models()) ?? []
    }

    public func refreshAudioInputs() async {
        audioInputs = await core.audioDevices.inputs()
    }

    // MARK: Language

    /// Applied on selection, not on Save.
    ///
    /// A language control that needs a second click to take effect leaves the
    /// user reading the language they were trying to leave.
    public func setLocale(_ next: UILocale) {
        locale = next
        Task { try? await core.settings.set(SettingKey.uiLocale, to: next.rawValue) }
    }

    // MARK: Activity transitions

    public func beginRecording() {
        guard activity != .recording else { return }
        elapsed = 0
        partial = ""
        levels = Array(repeating: 0, count: Self.levelCount)
        status = .statusListening
        withAnimation(Motion.expand) { activity = .recording }
    }

    public func beginTranscribing() {
        guard activity == .recording else { return }
        status = .statusTranscribing
        withAnimation(Motion.content) { activity = .transcribing }
    }

    public func finish(with transcript: String) {
        lastTranscript = transcript
        partial = ""
        status = .statusSaved
        withAnimation(Motion.expand) { activity = .idle }
    }

    public func abandon() {
        partial = ""
        status = nil
        withAnimation(Motion.collapse) { activity = .idle }
    }

    public func push(level newLevel: CGFloat) {
        levels.removeFirst()
        levels.append(newLevel)
    }

    // MARK: Persisting settings

    public func save(_ key: String, _ value: String) async {
        try? await core.settings.set(key, to: value)
    }
}

/// Motion constants.
///
/// Damping ratio + response, following Apple's fluid-interface convention
/// rather than mass/stiffness/damping. Response is not a duration — a spring has
/// no fixed duration, its settle time falls out of the parameters.
///
/// Expand carries a hint of overshoot (0.82) because a key press is an impulse
/// and the material should read as springing open. Collapse is near-critically
/// damped (0.92) because releasing a key is the *end* of an impulse; bounce
/// there would read as the UI having an opinion of its own.
public enum Motion {
    public static let expand = Animation.spring(response: 0.38, dampingFraction: 0.82)
    public static let collapse = Animation.spring(response: 0.32, dampingFraction: 0.92)
    /// Content swap inside a surface whose size is already moving.
    public static let content = Animation.spring(response: 0.30, dampingFraction: 1.0)
}
