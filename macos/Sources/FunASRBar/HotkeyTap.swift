import AppKit
import ApplicationServices
import CoreGraphics

/// Global push-to-talk on ⌃⌥Space via `CGEventTap`.
///
/// A tap rather than `RegisterEventHotKey` because push-to-talk needs the *key
/// up* as much as the key down, and Carbon hot keys only report the press.
///
/// Needs Accessibility. Note the rebuild trap on this machine: an ad-hoc
/// signature changes on every build, so macOS treats each build as a different
/// program and silently drops the old grant. The symptom is a checked checkbox
/// in System Settings that does nothing. The fix is to remove the stale entry
/// with − and re-add the app with +.
final class HotkeyTap {
    private var tap: CFMachPort?
    private var source: CFRunLoopSource?
    private var isDown = false

    var onPress: (() -> Void)?
    var onRelease: (() -> Void)?

    private static let spaceKeyCode: Int64 = 49
    private static let requiredFlags: CGEventFlags = [.maskControl, .maskAlternate]

    /// Cached: `AXIsProcessTrusted()` is a synchronous IPC to tccd, and polling
    /// it for a status file was enough to fill the system log with
    /// `TCCAccessRequest()` at 5 Hz.
    private(set) var isTrusted: Bool = AXIsProcessTrusted()

    @discardableResult
    func refreshTrust() -> Bool {
        isTrusted = AXIsProcessTrusted()
        return isTrusted
    }

    @discardableResult
    func start() -> Bool {
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
            options: .defaultTap,          // not listenOnly: the Space must not
                                           // also be typed into the focused app
            eventsOfInterest: mask,
            callback: { _, type, event, refcon in
                guard let refcon else { return Unmanaged.passUnretained(event) }
                let me = Unmanaged<HotkeyTap>.fromOpaque(refcon).takeUnretainedValue()
                return me.handle(type: type, event: event)
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
        NSLog("[hotkey] event tap armed on ⌃⌥Space")
        return true
    }

    func stop() {
        if let tap { CGEvent.tapEnable(tap: tap, enable: false) }
        if let source { CFRunLoopRemoveSource(CFRunLoopGetMain(), source, .commonModes) }
        tap = nil
        source = nil
    }

    private func handle(type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
        // The system disables a tap that blocks for too long. Re-arm rather than
        // going quietly dead.
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return Unmanaged.passUnretained(event)
        }

        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
        let flags = event.flags

        switch type {
        case .keyDown:
            guard keyCode == Self.spaceKeyCode,
                  flags.contains(.maskControl), flags.contains(.maskAlternate) else { break }
            // Ignore auto-repeat: holding is one press, not fifty.
            if event.getIntegerValueField(.keyboardEventAutorepeat) != 0 { return nil }
            if !isDown {
                isDown = true
                DispatchQueue.main.async { self.onPress?() }
            }
            return nil   // swallow

        case .keyUp:
            guard keyCode == Self.spaceKeyCode else { break }
            if isDown {
                isDown = false
                DispatchQueue.main.async { self.onRelease?() }
            }
            return nil   // swallow

        case .flagsChanged:
            // Letting go of ⌃ or ⌥ before Space ends the utterance too;
            // otherwise a sloppy release leaves the bar recording forever.
            if isDown, !(flags.contains(.maskControl) && flags.contains(.maskAlternate)) {
                isDown = false
                DispatchQueue.main.async { self.onRelease?() }
            }

        default:
            break
        }
        return Unmanaged.passUnretained(event)
    }
}
