import AppKit
import SwiftUI

/// A panel that appears without ever taking the keyboard away from whatever the
/// user is dictating into.
///
/// The whole product depends on this one property: the text is pasted back into
/// the app that had focus when the hotkey went down. If showing the bar steals
/// focus, there is nowhere for the text to go. Three things buy it, and all
/// three are required:
///
///   1. `.nonactivatingPanel` in the style mask — the panel can be clicked
///      without its application being activated.
///   2. `canBecomeKey`/`canBecomeMain` forced to `false` — nothing inside can
///      ever pull first-responder status.
///   3. `LSUIElement` + `.accessory` activation policy — the process has no
///      Dock tile and is never eligible to become the active app.
///
/// `orderFrontRegardless()` rather than `makeKeyAndOrderFront(_:)` is the
/// matching call: it shows the window without a key-window request.
final class VoiceBarPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    init(contentRect: NSRect) {
        super.init(
            contentRect: contentRect,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )

        isFloatingPanel = true
        becomesKeyOnlyIfNeeded = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        worksWhenModal = true
        isMovableByWindowBackground = false

        // Above normal windows and full-screen apps, below the menu-bar owner.
        level = .statusBar

        collectionBehavior = [
            .canJoinAllSpaces,      // follow the user across Spaces
            .fullScreenAuxiliary,   // and into another app's full screen
            .stationary,            // don't slide during Exposé
            .ignoresCycle           // never a ⌘` target
        ]

        // Any opaque surface anywhere in this chain kills the glass: the
        // material samples what is behind the *window*, so the window must have
        // nothing of its own to sample.
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false   // Liquid Glass casts its own; a window shadow would
                            // trace the transparent rectangle, not the bar.
    }
}

/// Owns the panel, keeps it positioned, and makes the transparent area of the
/// window click-through.
///
/// The window is deliberately larger than the bar and fixed in size: resizing it
/// per frame during the morph would put a window-server resize in the middle of
/// a spring and show up as tearing. The cost is a large invisible rectangle that
/// would otherwise swallow clicks, so `ignoresMouseEvents` is toggled from a
/// 30 Hz poll of the pointer against the bar's real rect.
@MainActor
final class BarWindowController {
    let panel: VoiceBarPanel
    private let model: BarModel
    private var pointerPoll: Timer?

    static let windowSize = NSSize(width: 760, height: 300)

    init(model: BarModel) {
        self.model = model
        let screen = NSScreen.main ?? NSScreen.screens[0]
        let vf = screen.visibleFrame
        let origin = NSPoint(
            x: vf.midX - Self.windowSize.width / 2,
            y: vf.minY + 96
        )
        panel = VoiceBarPanel(contentRect: NSRect(origin: origin, size: Self.windowSize))

        let host = NSHostingView(rootView: BarView(model: model))
        host.frame = NSRect(origin: .zero, size: Self.windowSize)
        host.autoresizingMask = [.width, .height]
        // NSHostingView is transparent unless SwiftUI paints something; make
        // sure no layer background sneaks in underneath the glass.
        host.wantsLayer = true
        host.layer?.backgroundColor = NSColor.clear.cgColor
        panel.contentView = host

        // Deliberately NOT shown here.
        //
        // This panel sits at `.statusBar`, above every window the user owns. On
        // a machine somebody is actually working on, a prototype that puts
        // itself on top of everything the moment it launches is not a prototype,
        // it is an interruption. Nothing appears until something asks for it.
    }

    func show() {
        guard !panel.isVisible else { return }
        panel.orderFrontRegardless()   // never makeKeyAndOrderFront
        startPointerPoll()
    }

    func hide() {
        pointerPoll?.invalidate()
        pointerPoll = nil
        panel.orderOut(nil)
    }

    var isVisible: Bool { panel.isVisible }

    /// Bar rect in screen coordinates (AppKit: origin bottom-left).
    var barScreenRect: NSRect? {
        guard let r = model.barRectInView else { return nil }
        let f = panel.frame
        return NSRect(
            x: f.minX + r.minX,
            y: f.maxY - r.maxY,
            width: r.width,
            height: r.height
        )
    }

    private func startPointerPoll() {
        pointerPoll = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                guard let bar = self.barScreenRect else { return }
                let p = NSEvent.mouseLocation
                let inside = bar.insetBy(dx: -6, dy: -6).contains(p)  // ~10px of hit padding
                if self.panel.ignoresMouseEvents == inside {
                    self.panel.ignoresMouseEvents = !inside
                }
            }
        }
    }

    func setAppearance(_ name: NSAppearance.Name?) {
        let appearance = name.flatMap { NSAppearance(named: $0) }
        NSApp.appearance = appearance
        panel.appearance = appearance
    }

    /// Move the bar's bottom edge to a given offset above the visible bottom of
    /// the screen.
    func reposition(bottomInset: CGFloat) {
        let screen = NSScreen.main ?? NSScreen.screens[0]
        let vf = screen.visibleFrame
        panel.setFrameOrigin(NSPoint(
            x: vf.midX - Self.windowSize.width / 2,
            y: vf.minY + bottomInset
        ))
    }
}
