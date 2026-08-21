import KoushuCore
import SwiftUI

/// The main window: sessions on the left, transcripts on the right, and the
/// controls that decide what the next recording does along the bottom.
///
/// `NavigationSplitView` rather than an HStack with a divider. It is not a
/// shortcut — it is what makes the sidebar an actual sidebar: the system gives
/// it its own material, the toolbar spans it correctly, the collapse control
/// works, and the whole thing keeps the proportions every other Mac app has. An
/// HStack would have to reimplement all of that and would still not match.
struct MainWindowView: View {
    @Bindable var app: AppModel
    @Bindable var browser: SessionBrowser

    var body: some View {
        NavigationSplitView {
            SessionSidebar(app: app, browser: browser)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 340)
        } detail: {
            detail
                // The deck floats over the content rather than consuming a strip
                // of it: transcripts scroll underneath, which is the one place
                // in this window where a translucent surface has something to be
                // translucent *about*.
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    ControlDeck(app: app, browser: browser)
                }
        }
        .searchable(
            text: $browser.query,
            placement: .sidebar,
            prompt: Text(app.l(.searchPlaceholder))
        )
        .navigationTitle(browser.activeSession?.title ?? app.l(.noSession))
        .navigationSubtitle(subtitle)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    NotificationCenter.default.post(name: .koushuToggleVoiceBar, object: nil)
                } label: {
                    Label(app.l(.showVoiceBar), systemImage: "rectangle.bottomhalf.inset.filled")
                }
                .help(app.l(.showVoiceBar))
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    NotificationCenter.default.post(name: .koushuOpenSettings, object: nil)
                } label: {
                    Label(app.l(.settings), systemImage: "gearshape")
                }
                .help(app.l(.settings))
            }
        }
        .environment(\.locale, Locale(identifier: app.locale == .zh ? "zh-Hans" : "en"))
    }

    /// One line of status, in the window's subtitle.
    ///
    /// Text from the core outranks our own labels: a sentence explaining why
    /// nothing was inserted matters more than the word "Saved".
    private var subtitle: String {
        if !app.coreMessage.isEmpty { return app.coreMessage }
        return app.l(app.status ?? .statusReady)
    }

    @ViewBuilder
    private var detail: some View {
        // Search takes over the pane rather than opening beside it: a hit is a
        // transcript, and this is where transcripts are read. Clearing the box
        // puts the session straight back, untouched.
        if browser.isSearching {
            SearchResultsView(app: app, browser: browser)
        } else {
            TranscriptPane(app: app, browser: browser)
        }
    }
}

extension Notification.Name {
    /// The window is SwiftUI and the panel is AppKit, and neither owns the
    /// other. A notification is the smallest thing that lets a toolbar button
    /// reach the delegate without either side holding a reference to it.
    static let koushuToggleVoiceBar = Notification.Name("koushu.toggleVoiceBar")
    static let koushuOpenSettings = Notification.Name("koushu.openSettings")
}
