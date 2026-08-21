import AppKit
import KoushuCore
import SwiftUI

/// Opens and closes the app's two real windows.
///
/// Written against AppKit rather than as a SwiftUI `App` with `Window` scenes,
/// for one reason that decides everything else: this process must be able to
/// exist with **no** window and no Dock tile, because that is what it does for
/// almost all of its life. A SwiftUI `App` wants a scene graph that is present
/// from launch, and the shapes that make it not show anything are the shapes
/// that make it hard to be sure it never will.
///
/// The activation policy is switched on demand. An `.accessory` process cannot
/// take focus or own a menu bar, which is exactly right for the voice bar and
/// exactly wrong for a window with a text field in it. So a window opening
/// promotes the app to `.regular`, and the last one closing demotes it back —
/// after which the app is again something that cannot steal the keyboard.
@MainActor
final class WindowManager {
    private let app: AppModel
    private let browser: SessionBrowser

    private var mainWindow: NSWindow?
    private var settingsWindow: NSWindow?
    private var loaded = false

    init(app: AppModel, browser: SessionBrowser) {
        self.app = app
        self.browser = browser
    }

    var hasVisibleWindow: Bool {
        (mainWindow?.isVisible ?? false) || (settingsWindow?.isVisible ?? false)
    }

    // MARK: Opening

    func showMain() {
        Task { await loadOnce() }
        let window = mainWindow ?? makeMainWindow()
        mainWindow = window
        present(window)
    }

    func showSettings() {
        Task { await loadOnce() }
        let window = settingsWindow ?? makeSettingsWindow()
        settingsWindow = window
        present(window)
    }

    /// Read the database the first time a window is opened, not at launch.
    ///
    /// A process that has shown nothing has no reason to have touched storage,
    /// and doing it lazily is what keeps a bare launch genuinely inert.
    private func loadOnce() async {
        guard !loaded else { return }
        loaded = true
        await app.load()
        await browser.start()
    }

    private func present(_ window: NSWindow) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    // MARK: Construction

    private func makeMainWindow() -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1_100, height: 720),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = app.l(.appName)
        window.titlebarAppearsTransparent = true
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 820, height: 520)
        window.contentView = NSHostingView(rootView: MainWindowView(app: app, browser: browser))
        window.center()
        window.setFrameAutosaveName("koushu.main")
        window.delegate = closeWatcher
        return window
    }

    private func makeSettingsWindow() -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 840, height: 620),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = app.l(.settingsTitle)
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 680, height: 460)
        window.contentView = NSHostingView(rootView: SettingsWindowView(app: app))
        window.center()
        window.setFrameAutosaveName("koushu.settings")
        window.delegate = closeWatcher
        return window
    }

    // MARK: Closing

    private lazy var closeWatcher = WindowCloseWatcher { [weak self] in
        self?.windowDidClose()
    }

    /// Closing the last window does not quit.
    ///
    /// This app lives in the menu bar; the window is not what it is. Quitting is
    /// one click away in that menu, which is where an app that lives there is
    /// expected to put it. What does happen is the demotion back to `.accessory`
    /// — otherwise the app would sit in the Dock forever after being opened
    /// once, and would still be eligible to take focus from the app the user is
    /// dictating into.
    private func windowDidClose() {
        // Deferred: `windowWillClose` fires before the window actually goes
        // away, so asking now would always see it as still visible.
        DispatchQueue.main.async { [weak self] in
            guard let self, !hasVisibleWindow else { return }
            NSApp.setActivationPolicy(.accessory)
        }
    }
}

private final class WindowCloseWatcher: NSObject, NSWindowDelegate {
    private let onClose: () -> Void
    init(onClose: @escaping () -> Void) {
        self.onClose = onClose
    }

    func windowWillClose(_ notification: Notification) {
        onClose()
    }
}
