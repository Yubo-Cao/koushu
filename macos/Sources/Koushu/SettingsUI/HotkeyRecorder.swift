import AppKit
import KoushuCore
import SwiftUI

/// Records a push-to-talk chord by having the user press it.
///
/// Nobody should have to type `CTRL+ALT+space`, so nothing asks them to. Two
/// things make this harder than it looks:
///
///   * **The global tap has to let go while recording.** It is armed on the
///     current chord and swallows it, so without suspending it, pressing the
///     shortcut you are trying to replace starts a recording instead of being
///     recorded.
///   * **The key must be read from its physical code, not its character.** With
///     Control and Option held, the character a key produces depends on the
///     layout — Ctrl+Alt+2 arrives as `@` on some and as a dead key on others —
///     so a chord recorded by character would depend on which modifiers happened
///     to be down while recording it.
struct HotkeyRecorder: View {
    @Bindable var app: AppModel

    /// Suspends and re-arms the tap around a recording. Owned by the delegate,
    /// because it is the thing that owns the tap.
    var suspend: () -> Void
    var apply: (Chord) -> Void

    @State private var recording = false
    @State private var pending: [Chord.Modifier] = []
    @State private var problem: Chord.Problem?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                field
                if recording {
                    Button(app.l(.hotkeyChange)) { stop() }
                        .keyboardShortcut(.cancelAction)
                        .hidden()
                } else {
                    Button(app.l(.hotkeyChange)) { start() }
                    Button(app.l(.hotkeyReset)) {
                        apply(.default)
                    }
                    .disabled(app.chord == .default)
                }
            }

            Text(hint)
                .font(.caption)
                .foregroundStyle(hintIsProblem ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
                .fixedSize(horizontal: false, vertical: true)

            if !app.accessibilityTrusted {
                // The one failure this control cannot fix by itself. Saying
                // "not listening" without saying why would send the user
                // hunting for a bug in the shortcut.
                HStack(spacing: 8) {
                    Text(app.l(.hotkeyNeedsAccessibility))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Button(app.l(.hotkeyOpenAccessibility)) {
                        HotkeyTap.promptForAccessibility()
                        HotkeyTap.openAccessibilitySettings()
                    }
                    .controlSize(.small)
                }
            }
        }
    }

    // MARK: The field

    private var field: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(.quinary)
                .strokeBorder(recording ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: 1)
            Text(fieldText)
                .font(.system(size: 13, weight: .medium))
                .monospacedDigit()
                .foregroundStyle(recording ? AnyShapeStyle(.secondary) : AnyShapeStyle(.primary))
                .padding(.horizontal, 10)
        }
        .frame(width: 160, height: 26)
        .overlay {
            if recording {
                KeyCaptureView(
                    onModifiers: { pending = $0 },
                    onChord: { modifiers, key in complete(modifiers, key) },
                    onCancel: { stop() }
                )
            }
        }
    }

    private var fieldText: String {
        if recording {
            return pending.isEmpty
                ? app.l(.hotkeyRecording)
                : pending.map(\.glyph).joined()
        }
        return app.chord.display(spaceLabel: app.l(.hotkeyKeySpace))
    }

    private var hintIsProblem: Bool {
        problem != nil || (!recording && !app.hotkeyArmed)
    }

    private var hint: String {
        if let problem {
            switch problem {
            case .needsModifier: return app.l(.hotkeyNeedsModifier)
            case .unsupportedKey: return app.l(.hotkeyUnsupportedKey)
            }
        }
        if recording { return app.l(.hotkeyRecordingHint) }
        // Not "failed to save": the choice is stored either way, and what the
        // user needs to know is that holding the key will do nothing.
        if !app.hotkeyArmed { return app.l(.hotkeyNotBound) }
        return app.l(.hotkeyLive(chord: app.chord.display(spaceLabel: app.l(.hotkeyKeySpace))))
    }

    // MARK: Recording

    private func start() {
        problem = nil
        pending = []
        recording = true
        suspend()
    }

    private func stop() {
        recording = false
        pending = []
        // Re-arm on whatever is current, so cancelling leaves the old shortcut
        // working rather than leaving the app deaf.
        apply(app.chord)
    }

    private func complete(_ modifiers: [Chord.Modifier], _ key: String) {
        if let refusal = Chord.validate(modifiers: modifiers, key: key) {
            problem = refusal
            pending = modifiers
            return
        }
        problem = nil
        recording = false
        pending = []
        apply(Chord(modifiers: modifiers, key: key))
    }
}

/// A first-responder view that reports raw key events.
///
/// SwiftUI's `onKeyPress` deals in characters and swallows the modifier-only
/// transitions this recorder shows while a chord is being assembled, so the
/// capture drops to AppKit.
private struct KeyCaptureView: NSViewRepresentable {
    var onModifiers: ([Chord.Modifier]) -> Void
    var onChord: ([Chord.Modifier], String) -> Void
    var onCancel: () -> Void

    func makeNSView(context: Context) -> CaptureView {
        let view = CaptureView()
        view.onModifiers = onModifiers
        view.onChord = onChord
        view.onCancel = onCancel
        return view
    }

    func updateNSView(_ view: CaptureView, context: Context) {
        view.onModifiers = onModifiers
        view.onChord = onChord
        view.onCancel = onCancel
        DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
    }

    final class CaptureView: NSView {
        var onModifiers: (([Chord.Modifier]) -> Void)?
        var onChord: (([Chord.Modifier], String) -> Void)?
        var onCancel: (() -> Void)?

        override var acceptsFirstResponder: Bool { true }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            window?.makeFirstResponder(self)
        }

        override func flagsChanged(with event: NSEvent) {
            // Pressing a modifier reports progress rather than an error, which
            // is what makes this feel like a recorder: the glyphs appear as the
            // keys go down, and the chord completes on the key that finishes it.
            onModifiers?(Self.modifiers(from: event))
        }

        override func keyDown(with event: NSEvent) {
            // Escape cancels. It is also the reason Escape is not a bindable
            // key: it has to keep meaning "get me out of here".
            if event.keyCode == 53 {
                onCancel?()
                return
            }
            let modifiers = Self.modifiers(from: event)
            guard let key = Chord.keyName(for: Int64(event.keyCode)) else {
                onChord?(modifiers, "\u{0}")   // refused, and says why
                return
            }
            onChord?(modifiers, key)
        }

        override func performKeyEquivalent(with event: NSEvent) -> Bool {
            // Without this, a chord containing Command is eaten by the menu bar
            // before it ever reaches `keyDown`, and ⌘-anything cannot be
            // recorded at all.
            guard window?.firstResponder === self, event.type == .keyDown else { return false }
            keyDown(with: event)
            return true
        }

        static func modifiers(from event: NSEvent) -> [Chord.Modifier] {
            var result: [Chord.Modifier] = []
            let flags = event.modifierFlags
            if flags.contains(.control) { result.append(.control) }
            if flags.contains(.option) { result.append(.option) }
            if flags.contains(.shift) { result.append(.shift) }
            if flags.contains(.command) { result.append(.command) }
            return result
        }
    }
}
