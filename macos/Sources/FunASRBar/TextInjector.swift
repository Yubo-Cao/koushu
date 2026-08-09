import AppKit
import CoreGraphics

/// Put the transcript into whatever application currently has the keyboard.
///
/// This is the payoff for the non-activating panel, and the reason focus
/// discipline is the load-bearing requirement rather than a nicety: the text has
/// to arrive in the app the user was already typing in, so that app must still
/// be frontmost when the utterance ends.
///
/// `CGEventKeyboardSetUnicodeString` synthesises the characters directly instead
/// of going through the clipboard. That matters for three reasons the clipboard
/// approach cannot fix:
///
///   1. It does not destroy the user's pasteboard. The Tauri build has to save
///      the clipboard, overwrite it, paste, and restore it — a sequence that
///      races with any clipboard manager the user runs.
///   2. It does not depend on the target app implementing ⌘V, or on the chord
///      table being right for the current keyboard layout.
///   3. It carries arbitrary Unicode, so Chinese needs no special casing.
///
/// Requires Accessibility. Never call this on a whim: it types into whatever is
/// focused, which on somebody's working machine is their real document.
enum TextInjector {

    /// `CGEventKeyboardSetUnicodeString` is documented as taking a short
    /// string; long buffers get silently truncated by some receivers, so the
    /// text is posted in small chunks.
    private static let chunkSize = 16

    static func insert(_ text: String) {
        guard AXIsProcessTrusted() else {
            NSLog("[inject] refused: no Accessibility grant")
            return
        }
        guard !text.isEmpty else { return }

        let source = CGEventSource(stateID: .privateState)
        let units = Array(text.utf16)
        var index = 0

        while index < units.count {
            let end = min(index + chunkSize, units.count)
            var chunk = Array(units[index..<end])

            // virtualKey 0 with a Unicode payload: the keycode is ignored and
            // the string is what gets delivered.
            guard let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
                  let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false)
            else { return }

            down.keyboardSetUnicodeString(stringLength: chunk.count, unicodeString: &chunk)
            up.keyboardSetUnicodeString(stringLength: chunk.count, unicodeString: &chunk)

            down.post(tap: .cgAnnotatedSessionEventTap)
            up.post(tap: .cgAnnotatedSessionEventTap)

            index = end
            // A short gap: posting a long burst with no spacing drops characters
            // in apps that coalesce key events.
            usleep(1500)
        }
        NSLog("[inject] posted \(units.count) UTF-16 units")
    }

    /// The app that will receive the text — i.e. the one that still has focus
    /// because the bar refused to take it.
    static var focusTargetDescription: String {
        let front = NSWorkspace.shared.frontmostApplication
        return front?.bundleIdentifier ?? front?.localizedName ?? "none"
    }
}
