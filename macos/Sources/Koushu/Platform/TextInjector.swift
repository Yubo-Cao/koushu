import AppKit
import CoreGraphics

/// The application a transcript will be inserted into.
///
/// Captured when push-to-talk *starts*, not when the text is ready. By the time
/// the words exist the user may have switched windows, and inserting a
/// transcript into the wrong application is worse than not inserting it.
struct InjectTarget: Equatable, Sendable {
    var bundleID: String?
    var name: String?
    var pid: pid_t?

    var description: String { bundleID ?? name ?? "none" }

    static var current: InjectTarget {
        let front = NSWorkspace.shared.frontmostApplication
        return InjectTarget(
            bundleID: front?.bundleIdentifier,
            name: front?.localizedName,
            pid: front?.processIdentifier
        )
    }
}

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

    enum Refusal: Equatable {
        /// No Accessibility grant, so posting events would do nothing anyway.
        case notTrusted
        /// The user switched applications between pressing the key and letting
        /// go. Typing into the new one is the wrong outcome, so nothing is sent
        /// and the transcript stays where it can still be copied.
        case targetChanged(from: String, to: String)
    }

    /// `CGEventKeyboardSetUnicodeString` is documented as taking a short string;
    /// long buffers get silently truncated by some receivers, so the text is
    /// posted in small chunks.
    private static let chunkSize = 16

    @discardableResult
    static func insert(_ text: String, into target: InjectTarget?) -> Refusal? {
        guard AXIsProcessTrusted() else {
            NSLog("[inject] refused: no Accessibility grant")
            return .notTrusted
        }
        guard !text.isEmpty else { return nil }

        if let target, let expected = target.pid {
            let front = NSWorkspace.shared.frontmostApplication
            if let actual = front?.processIdentifier, actual != expected {
                NSLog("[inject] refused: focus moved \(target.description) -> \(front?.bundleIdentifier ?? "?")")
                return .targetChanged(
                    from: target.description,
                    to: front?.bundleIdentifier ?? front?.localizedName ?? "?"
                )
            }
        }

        let source = CGEventSource(stateID: .privateState)
        let units = Array(text.utf16)
        var index = 0

        while index < units.count {
            let end = min(index + chunkSize, units.count)
            var chunk = Array(units[index..<end])

            // virtualKey 0 with a Unicode payload: the key code is ignored and
            // the string is what gets delivered.
            guard let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
                  let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false)
            else { return nil }

            down.keyboardSetUnicodeString(stringLength: chunk.count, unicodeString: &chunk)
            up.keyboardSetUnicodeString(stringLength: chunk.count, unicodeString: &chunk)

            down.post(tap: .cgAnnotatedSessionEventTap)
            up.post(tap: .cgAnnotatedSessionEventTap)

            index = end
            // A short gap: posting a long burst with no spacing drops characters
            // in apps that coalesce key events.
            usleep(1500)
        }
        NSLog("[inject] posted \(units.count) UTF-16 units to \(target?.description ?? "focus")")
        return nil
    }

    /// Put it on the clipboard as well, so a failed insertion is recoverable.
    ///
    /// Not instead of inserting: the whole point is that the words land where
    /// the user was typing. But an app that refuses synthesised key events, or a
    /// focus change mid-utterance, would otherwise lose the transcript entirely.
    static func copyToPasteboard(_ text: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }
}
