import AVFoundation
import AppKit
import ApplicationServices
import KoushuCore
import SwiftUI

/// Wires the pieces together, and decides what a bare launch is allowed to do.
///
/// **Nothing intrusive happens without being asked for.** This runs on somebody's
/// working machine, so the safe default is a process that launches, shows no
/// window, grabs no keys, opens no microphone and waits. A bare launch is
/// indistinguishable from not launching it at all except for one icon in the
/// menu bar, which is the whole point of the icon.
///
/// Everything beyond that is opt-in, either by the user clicking something or by
/// a launch flag:
///
/// | Flag | What it turns on |
/// |---|---|
/// | `--hotkey` | arms the global event tap on the stored chord |
/// | `--menubar` | installs the menu-bar item |
/// | `--show` | shows the voice bar |
/// | `--main` / `--settings` | opens a window |
/// | `--mic` | asks for microphone access |
///
/// The flags exist because the screenshot rig has to put the app into a given
/// state from a shell, and because a global key grab is not something to arm by
/// default on a machine where somebody is typing.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let core = CoreFactory.make()
    private(set) var app: AppModel!
    private var browser: SessionBrowser!
    private var windows: WindowManager!
    private var statusItem: StatusItemController!
    private var utterance: UtteranceController!
    private var audio: AudioLevelMonitor!
    private var barController: BarWindowController!
    private let bar = VoiceBarModel()
    private let hotkey = HotkeyTap()
    private let control = ControlChannel()
    private let backdrop = BackdropController()

    private var args: Set<String> { Set(CommandLine.arguments.dropFirst()) }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        app = AppModel(core: core)
        browser = SessionBrowser(core: core)
        audio = AudioLevelMonitor(model: app)
        utterance = UtteranceController(app: app, audio: audio, browser: browser)
        windows = WindowManager(app: app, browser: browser)
        barController = BarWindowController(app: app, bar: bar)

        statusItem = StatusItemController(app: app)
        statusItem.onOpenMain = { [weak self] in self?.windows.showMain() }
        statusItem.onOpenSettings = { [weak self] in self?.windows.showSettings() }

        app.accessibilityTrusted = hotkey.refreshTrust()
        app.microphoneGranted = AudioLevelMonitor.authorization == .authorized ? true : nil

        observeNotifications()
        startStatusItemSync()

        hotkey.onPress = { [weak self] in self?.pressed() }
        hotkey.onRelease = { [weak self] in self?.released() }

        // Settings are read before anything is armed, so `--hotkey` arms the
        // chord the user chose rather than the default and then correcting
        // itself a moment later.
        Task {
            await app.load()
            applyLaunchFlags()
        }

        control.handler = { [weak self] command in self?.run(command) }
        control.statusProvider = { [weak self] in self?.status() ?? [:] }
        control.start()

        NSLog("[koushu] up, pid=\(getpid()) accessory=true")
    }

    private func applyLaunchFlags() {
        let args = args

        if args.contains("--menubar") { statusItem.install() }

        // A global event tap swallows the chord system-wide. Arming it by
        // default would take keys out from under whatever the user is typing
        // into, for an app they have not asked to have listening.
        if args.contains("--hotkey") {
            app.hotkeyArmed = hotkey.arm(app.chord)
        }

        // Asking for the microphone puts a system dialog on screen, so it is
        // only requested when something is actually going to record.
        if args.contains("--mic") {
            Task {
                app.microphoneGranted = await audio.requestAccess()
            }
        }

        if args.contains("--show") { barController.show() }
        if args.contains("--main") { windows.showMain() }
        if args.contains("--settings") { windows.showSettings() }
    }

    /// The menu-bar item follows the model rather than a timer.
    ///
    /// `withObservationTracking` re-registers itself after each change, which is
    /// the supported way to observe an `@Observable` from outside SwiftUI. A
    /// 350 ms poll — which is what the Tauri tray has to do — would be visible
    /// as a lag between the key going down and the icon changing.
    private func startStatusItemSync() {
        withObservationTracking {
            statusItem.refresh()
        } onChange: { [weak self] in
            // Re-registers rather than recursing: `withObservationTracking`
            // fires once per change and then stops watching, so the next
            // registration has to happen after the change has been applied.
            Task { @MainActor in self?.startStatusItemSync() }
        }
    }

    private func observeNotifications() {
        let center = NotificationCenter.default
        center.addObserver(forName: .koushuToggleVoiceBar, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.barController.isVisible ? self.barController.hide() : self.barController.show()
            }
        }
        center.addObserver(forName: .koushuOpenSettings, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.windows.showSettings() }
        }
        center.addObserver(forName: .koushuSuspendHotkey, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                // Let go of the binding while a new chord is being recorded, so
                // pressing the current one records it instead of starting a
                // recording.
                self?.hotkey.stop()
                self?.app.hotkeyArmed = false
            }
        }
        center.addObserver(forName: .koushuApplyHotkey, object: nil, queue: .main) { [weak self] note in
            // Read out of the notification before crossing the isolation
            // boundary: `Notification` is not Sendable, but the string inside it
            // is, and the string is the whole payload.
            let stored = note.userInfo?["chord"] as? String
            MainActor.assumeIsolated {
                guard let self, let stored, let chord = Chord(stored: stored) else { return }
                self.applyChord(chord)
            }
        }
    }

    /// Store the chord and put it into effect, in that order.
    ///
    /// The choice is saved either way. Whether a listener is running is a
    /// separate question from whether the setting was written, and conflating
    /// them is how a shortcut ends up accepted and inert.
    private func applyChord(_ chord: Chord) {
        app.chord = chord
        Task { await app.save(SettingKey.pushToTalkTrigger, chord.stored) }
        app.accessibilityTrusted = hotkey.refreshTrust()
        app.hotkeyArmed = hotkey.arm(chord)
    }

    // MARK: Utterance

    /// The real push-to-talk key. This is the only path that may type.
    private func pressed() {
        barController.show()
        utterance.begin(deliversText: true)
    }

    private func released() {
        utterance.finish()
    }

    // MARK: Control channel

    private func status() -> [String: Any] {
        // Evidence for the one property the product depends on: the bar is on
        // screen and the keyboard still belongs to somebody else.
        let front = NSWorkspace.shared.frontmostApplication
        var report: [String: Any] = [
            "activity": app.activity.rawValue,
            "locale": app.locale.rawValue,
            "accessibility": hotkey.isTrusted,
            "hotkeyArmed": app.hotkeyArmed,
            "chord": app.chord.stored,
            "focus": [
                "weAreActive": NSApp.isActive,
                "panelIsKey": barController.panel.isKeyWindow,
                "panelCanBecomeKey": barController.panel.canBecomeKey,
                "panelIsVisible": barController.panel.isVisible,
                "frontmostApp": front?.bundleIdentifier ?? front?.localizedName ?? "?",
                "frontmostPid": front?.processIdentifier ?? -1,
                "ourPid": getpid(),
                "activationPolicy": NSApp.activationPolicy() == .accessory ? "accessory" : "regular",
            ],
            "micAuthorized": app.microphoneGranted ?? false,
            "micStatus": AudioLevelMonitor.authorization.rawValue,
            "level": Double(app.level),
            "levelPeakRecent": Double(app.levels.max() ?? 0),
            "backdrop": backdrop.kind.rawValue,
            "appearance": NSApp.effectiveAppearance.name.rawValue,
            "sessions": browser.sessions.count,
            "windows": windowReport(),
        ]
        if let rect = barController.barScreenRect {
            let primary = NSScreen.screens.first?.frame ?? .zero
            report["barScreen"] = ["x": rect.minX, "y": rect.minY, "w": rect.width, "h": rect.height]
            // `screencapture -R` wants a top-left origin with y growing down.
            report["barCapture"] = [
                "x": Int(rect.minX.rounded()),
                "y": Int((primary.height - rect.maxY).rounded()),
                "w": Int(rect.width.rounded()),
                "h": Int(rect.height.rounded()),
            ]
        }
        return report
    }

    /// Each visible window, in the coordinates `screencapture -R` wants —
    /// top-left origin, y growing down — so the screenshot rig never has to
    /// guess where a window ended up.
    private func windowReport() -> [String: Any] {
        let primary = NSScreen.screens.first?.frame ?? .zero
        var report: [String: Any] = [:]
        // Keyed by the autosave name, not the title: the title is localised, and
        // a screenshot script that looks for "Settings" finds nothing the moment
        // the app is switched to Chinese.
        for window in NSApp.windows where window.isVisible && !window.frameAutosaveName.isEmpty {
            let frame = window.frame
            report[window.frameAutosaveName] = [
                "x": Int(frame.minX.rounded()),
                "y": Int((primary.height - frame.maxY).rounded()),
                "w": Int(frame.width.rounded()),
                "h": Int(frame.height.rounded()),
            ]
        }
        return report
    }

    // swiftlint:disable:next cyclomatic_complexity
    private func run(_ command: String) {
        let parts = command.split(separator: " ", maxSplits: 1).map(String.init)
        let verb = parts.first ?? ""
        let argument = parts.count > 1 ? parts[1] : ""
        NSLog("[cmd] \(command)")

        switch verb {
        case "show": barController.show()
        case "hide": barController.hide()
        case "main": windows.showMain()
        case "settings": windows.showSettings()

        case "idle": utterance.cancel()
        // Explicitly *not* `pressed()`. A scripted recording is a rehearsal;
        // it shows the bar and runs the real microphone, and it must not type
        // into whatever the person at the machine happens to have open.
        case "record":
            barController.show()
            utterance.begin(deliversText: false)
        case "stop": released()

        case "locale":
            if let locale = UILocale(rawValue: argument) { app.setLocale(locale) }

        case "search":
            browser.query = argument

        case "filter":
            switch argument {
            case "archived": browser.filter.archived = .archived
            case "all": browser.filter.archived = .all
            case "none": browser.resetFilters()
            default: break
            }

        case "inject":
            // Never automatic. Typing into whoever has focus is exactly the
            // thing you do not do by accident on somebody's real desktop.
            let text = argument.isEmpty ? app.lastTranscript : argument
            TextInjector.insert(text, into: .current)

        case "text":
            audio.stop()
            app.finish(with: argument.isEmpty ? StubTranscriptionEngine.placeholder(for: app.defaultLanguage) : argument)

        case "backdrop":
            // `backdrop terminal below` puts it under our windows, for
            // photographing the sidebar and toolbar materials.
            let parts = argument.split(separator: " ").map(String.init)
            backdrop.show(
                BackdropKind(rawValue: parts.first ?? "") ?? .none,
                below: parts.count > 1 && parts[1] == "below"
            )

        case "appearance":
            switch argument {
            case "dark":
                setAppearance(.darkAqua)
            case "light":
                setAppearance(.aqua)
            default:
                setAppearance(nil)
            }

        case "pos":
            barController.reposition(bottomInset: CGFloat(Double(argument) ?? 96))

        case "ax-prompt":
            HotkeyTap.promptForAccessibility()

        case "ax-recheck":
            app.accessibilityTrusted = hotkey.refreshTrust()
            app.hotkeyArmed = hotkey.arm(app.chord)

        case "mic-recheck":
            Task { app.microphoneGranted = await audio.requestAccess() }

        case "retain":
            // Keep the recording after decoding it, so the WAV the runtime was
            // given can be listened to. The gap between "the meter moved" and
            // "the file is well-formed" is where a capture bug hides.
            app.retainAudio = argument != "0"

        case "transcribe":
            // Decode a file that already exists, through the same engine a real
            // utterance uses. Without it the only way to check recognition is to
            // speak into somebody's microphone and hope the room is quiet — and
            // a silent room produces "no speech", which proves nothing either
            // way. A fixed file makes the answer reproducible.
            let request = TranscriptionRequest(
                wavPath: argument,
                modelID: app.defaultModelID,
                backend: app.runtime,
                language: app.defaultLanguage
            )
            NSLog("[asr] decoding \(argument) with \(app.runtime)")
            _ = app.core.transcription.transcribe(request) { event in
                switch event {
                case .committed(let text, let elapsedMS):
                    NSLog("[asr] ok in \(elapsedMS) ms: \(text)")
                case .noSpeech:
                    NSLog("[asr] no speech in the recording")
                case .failed(let message):
                    NSLog("[asr] failed: \(message)")
                case .partial:
                    break
                }
            }

        case "license":
            // The only end-to-end check of the FFI seam there is: a string goes
            // into Rust, a struct comes back, and the sentence inside it is the
            // core's, not one this side invented.
            let info = app.core.license.verify(argument)
            NSLog("[license] valid=\(info.valid) detail=\(info.detail)")

        case "set":
            applyTunable(argument)

        case "quit":
            NSApp.terminate(nil)

        default:
            NSLog("[cmd] unknown: \(command)")
        }
    }

    private func setAppearance(_ name: NSAppearance.Name?) {
        let appearance = name.flatMap { NSAppearance(named: $0) }
        NSApp.appearance = appearance
        barController.setAppearance(name)
        backdrop.setAppearance(appearance)
        for window in NSApp.windows { window.appearance = appearance }
    }

    private func applyTunable(_ argument: String) {
        let pair = argument.split(separator: " ").map(String.init)
        guard pair.count == 2 else { return }
        let value = Double(pair[1]) ?? 0
        withAnimation(Motion.expand) {
            switch pair[0] {
            case "glassSpacing": bar.glassSpacing = CGFloat(value)
            case "stackSpacing": bar.stackSpacing = CGFloat(value)
            case "orbSize": bar.orbSize = CGFloat(value)
            case "clear": bar.clearStyle = value != 0
            case "tint": bar.tintWhileRecording = value != 0
            case "interactive": bar.interactiveGlass = value != 0
            default: break
            }
        }
    }
}
