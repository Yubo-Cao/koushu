import AppKit
import KoushuCore

// `@preconcurrency` for the two C frameworks, and only for them.
//
// `CGEvent` is a CoreFoundation type with no Sendable annotation and
// `kAXTrustedCheckOptionPrompt` is a global `var` holding a constant string;
// neither will ever be audited, and neither is actually shared across threads
// here — the tap's run-loop source is installed on the main run loop, so its
// callback and everything it touches are already on one thread. Marking the
// imports says that once, rather than sprinkling `nonisolated(unsafe)` through
// the code that uses them.
@preconcurrency import ApplicationServices
@preconcurrency import CoreGraphics

/// Global push-to-talk, via `CGEventTap`.
///
/// A tap rather than `RegisterEventHotKey` because push-to-talk needs the *key
/// up* as much as the key down, and Carbon hot keys only report the press.
///
/// Needs Accessibility, and note the rebuild trap on a locally-signed build: an
/// ad-hoc signature changes on every build, so macOS treats each build as a
/// different program and silently drops the old grant. The symptom is a ticked
/// checkbox in System Settings that does nothing. `sign-identity.sh` is what
/// avoids it; see the comment at the top of that script.
///
/// The tap's run-loop source is installed on the **main** run loop, so the
/// C callback already runs on the main thread. `MainActor.assumeIsolated` states
/// that rather than hopping through `DispatchQueue.main.async`, which would put
/// a queue round-trip between the physical key press and the microphone opening
/// — the one latency in this app the user can feel.
@MainActor
final class HotkeyTap {
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private var isDown = false

    var onPress: (() -> Void)?
    var onRelease: (() -> Void)?

    /// The chord being listened for. Changing it re-arms the tap.
    private(set) var chord: Chord = .default
    private var keyCode: Int64 = Chord.keyCode(for: "space") ?? 49
    private var requiredFlags: CGEventFlags = [.maskControl, .maskAlternate]

    /// Cached: `AXIsProcessTrusted()` is a synchronous IPC to tccd, and polling
    /// it for a status line was enough to fill the system log with
    /// `TCCAccessRequest()` at 5 Hz.
    private(set) var isTrusted: Bool = AXIsProcessTrusted()

    @discardableResult
    func refreshTrust() -> Bool {
        isTrusted = AXIsProcessTrusted()
        return isTrusted
    }

    /// Open the standard "grant Accessibility" alert.
    ///
    /// Only ever from an explicit action. It puts a system dialog on screen,
    /// which is not something to do to somebody who has not asked for it.
    static func promptForAccessibility() {
        _ = AXIsProcessTrustedWithOptions(
            [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
        )
    }

    static func openAccessibilitySettings() {
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        else { return }
        NSWorkspace.shared.open(url)
    }

    // MARK: Arming

    /// Listen for `chord`. Returns whether a listener is actually running —
    /// which is not the same question as whether this call threw.
    @discardableResult
    func arm(_ chord: Chord) -> Bool {
        // Re-arming rather than mutating: the tap filters by key code inside its
        // callback, and leaving a live tap pointed at a stale code is how a
        // shortcut ends up accepted and inert.
        stop()
        self.chord = chord
        keyCode = Chord.keyCode(for: chord.key) ?? keyCode
        requiredFlags = Self.flags(for: chord.modifiers)
        return start()
    }

    private static func flags(for modifiers: [Chord.Modifier]) -> CGEventFlags {
        var flags: CGEventFlags = []
        for modifier in modifiers {
            switch modifier {
            case .control: flags.insert(.maskControl)
            case .option: flags.insert(.maskAlternate)
            case .shift: flags.insert(.maskShift)
            case .command: flags.insert(.maskCommand)
            }
        }
        return flags
    }

    @discardableResult
    private func start() -> Bool {
        guard tap == nil else { return true }
        guard refreshTrust() else {
            NSLog("[hotkey] not trusted for Accessibility; tap not created")
            return false
        }

        let mask: CGEventMask =
            (1 << CGEventType.keyDown.rawValue) |
            (1 << CGEventType.keyUp.rawValue) |
            (1 << CGEventType.flagsChanged.rawValue)

        let refcon = Unmanaged.passUnretained(self).toOpaque()

        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,          // not listenOnly: the key must not
                                           // also reach the focused app
            eventsOfInterest: mask,
            callback: { _, type, event, refcon in
                guard let refcon else { return Unmanaged.passUnretained(event) }
                // Safe because the source below is on the main run loop.
                return MainActor.assumeIsolated {
                    let tap = Unmanaged<HotkeyTap>.fromOpaque(refcon).takeUnretainedValue()
                    return tap.handle(type: type, event: event)
                }
            },
            userInfo: refcon
        ) else {
            NSLog("[hotkey] tapCreate returned nil")
            return false
        }

        self.tap = tap
        let src = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        source = src
        CFRunLoopAddSource(CFRunLoopGetMain(), src, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        NSLog("[hotkey] event tap armed on \(chord.stored)")
        return true
    }

    func stop() {
        if let tap { CGEvent.tapEnable(tap: tap, enable: false) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
        tap = nil
        source = nil
        // A tap torn down mid-press would otherwise leave the app recording with
        // nothing left to tell it the key came up.
        if isDown {
            isDown = false
            onRelease?()
        }
    }

    var isArmed: Bool { tap != nil }

    // MARK: Handling

    private func handle(type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
        // The system disables a tap that blocks for too long. Re-arm rather than
        // going quietly dead.
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return Unmanaged.passUnretained(event)
        }

        let code = event.getIntegerValueField(.keyboardEventKeycode)
        let flags = event.flags

        switch type {
        case .keyDown:
            guard code == keyCode, flags.isSuperset(of: requiredFlags) else { break }
            // Ignore auto-repeat: holding is one press, not fifty.
            if event.getIntegerValueField(.keyboardEventAutorepeat) != 0 { return nil }
            if !isDown {
                isDown = true
                onPress?()
            }
            return nil   // swallow

        case .keyUp:
            guard code == keyCode else { break }
            if isDown {
                isDown = false
                onRelease?()
            }
            return nil   // swallow

        case .flagsChanged:
            // Letting go of a modifier before the key ends the utterance too;
            // otherwise a sloppy release leaves the bar recording forever.
            if isDown, !flags.isSuperset(of: requiredFlags) {
                isDown = false
                onRelease?()
            }

        default:
            break
        }
        return Unmanaged.passUnretained(event)
    }
}

extension CGEventFlags {
    func isSuperset(of other: CGEventFlags) -> Bool {
        rawValue & other.rawValue == other.rawValue
    }
}
