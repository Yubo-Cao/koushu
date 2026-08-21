import AppKit
import KoushuCore

/// The menu-bar item.
///
/// It has two jobs. The first is proof of life: this app is driven almost
/// entirely by a global key, the windows spend most of their time closed and the
/// voice bar only appears while you are talking, so without something in the
/// menu bar there is no way to tell a running instance from a crashed one.
///
/// The second is state. An icon that never changes proves the process exists but
/// says nothing about what it is doing, and the two things the user cannot
/// otherwise see are *the microphone is open* and *your words are still being
/// transcribed*. So there are three icons, and they differ by **shape**:
///
/// | State | Glyph | Meaning |
/// |---|---|---|
/// | Idle | outlined mic | running, not listening |
/// | Recording | filled mic | the microphone is open |
/// | Transcribing | waveform | audio is being turned into text |
///
/// Template images, so macOS tints them to match the menu bar in either
/// appearance. That is also why the difference is shape and not colour: a
/// template image is rendered from its alpha channel alone, so colour is not
/// information the platform will keep.
@MainActor
final class StatusItemController {
    private let app: AppModel
    private var item: NSStatusItem?
    private var statusLine: NSMenuItem?
    private var openItem: NSMenuItem?
    private var settingsItem: NSMenuItem?
    private var quitItem: NSMenuItem?
    private var shown: Activity?
    private var shownLocale: UILocale?

    var onOpenMain: (() -> Void)?
    var onOpenSettings: (() -> Void)?

    init(app: AppModel) {
        self.app = app
    }

    func install() {
        guard item == nil else { return }
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        // A disabled item is how a menu says "this is a label". A click target
        // that did nothing would just look broken.
        let status = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        status.isEnabled = false
        let open = NSMenuItem(title: "", action: #selector(openMain), keyEquivalent: "")
        open.target = self
        let settings = NSMenuItem(title: "", action: #selector(openSettings), keyEquivalent: ",")
        settings.target = self
        let quit = NSMenuItem(title: "", action: #selector(quit), keyEquivalent: "q")
        quit.target = self

        let menu = NSMenu()
        menu.addItem(status)
        menu.addItem(.separator())
        menu.addItem(open)
        menu.addItem(settings)
        menu.addItem(.separator())
        menu.addItem(quit)

        item.menu = menu
        self.item = item
        statusLine = status
        openItem = open
        settingsItem = settings
        quitItem = quit

        refresh()
    }

    /// Bring the icon and the menu into step with the app.
    ///
    /// Called from an observation of `AppModel` rather than a timer: the state
    /// it reflects is already an observable property, so polling would be
    /// re-deriving something that is pushed.
    func refresh() {
        guard let item else { return }
        let activity = app.activity
        let locale = app.locale

        if shown != activity {
            item.button?.image = Self.icon(for: activity)
            item.button?.image?.isTemplate = true
            shown = activity
        }
        if shownLocale != locale || statusLine?.title.isEmpty == true {
            openItem?.title = app.l(.trayOpen)
            settingsItem?.title = app.l(.settings) + "…"
            quitItem?.title = app.l(.trayQuit)
            shownLocale = locale
        }
        statusLine?.title = app.l(activity.statusMessage)
        item.button?.toolTip = "\(app.l(.appName)) · \(app.l(activity.statusMessage))"
    }

    private static func icon(for activity: Activity) -> NSImage? {
        let name: String
        switch activity {
        case .idle: name = "mic"
        case .recording: name = "mic.fill"
        case .transcribing: name = "waveform"
        }
        return NSImage(systemSymbolName: name, accessibilityDescription: nil)
    }

    @objc private func openMain() { onOpenMain?() }
    @objc private func openSettings() { onOpenSettings?() }
    @objc private func quit() { NSApp.terminate(nil) }
}

extension Activity {
    var statusMessage: Msg {
        switch self {
        case .idle: .trayIdle
        case .recording: .trayRecording
        case .transcribing: .trayTranscribing
        }
    }
}
