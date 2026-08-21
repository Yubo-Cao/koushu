import Foundation

/// A push-to-talk shortcut, and the rules about what may be one.
///
/// The stored spelling is the XDG shortcuts form — `CTRL+ALT+space` — even
/// though nothing on macOS uses XDG. That is deliberate: `hotkey.pushToTalk` is
/// a row in the same settings table the Tauri build reads, so a Mac that writes
/// a Cocoa-flavoured spelling would hand the Linux build a chord it cannot
/// parse. It is not a form anyone should have to type, and nothing in the UI
/// asks them to — the chord is recorded by pressing it, and shown by
/// ``Chord/display(mac:spaceLabel:)``.
public struct Chord: Hashable, Sendable {
    /// In the order a chord is written: CTRL, ALT, SHIFT, LOGO.
    public var modifiers: [Modifier]
    /// The XKB-style key name: a lowercase letter, a digit, `F1`…`F20`, or
    /// `space`.
    public var key: String

    public init(modifiers: [Modifier], key: String) {
        self.modifiers = Modifier.canonicalOrder.filter { modifiers.contains($0) }
        self.key = key
    }

    public enum Modifier: String, CaseIterable, Sendable {
        case control = "CTRL"
        case option = "ALT"
        case shift = "SHIFT"
        case command = "LOGO"

        static let canonicalOrder: [Modifier] = [.control, .option, .shift, .command]

        /// Apple's glyphs, in Apple's order.
        public var glyph: String {
            switch self {
            case .control: "⌃"
            case .option: "⌥"
            case .shift: "⇧"
            case .command: "⌘"
            }
        }
    }

    /// The chord the app ships with, and what "restore default" restores.
    public static let `default` = Chord(modifiers: [.control, .option], key: "space")

    // MARK: Wire form

    public var stored: String {
        (modifiers.map(\.rawValue) + [key]).joined(separator: "+")
    }

    public init?(stored: String) {
        var modifiers: [Modifier] = []
        var key: String?
        for part in stored.split(separator: "+").map(String.init) where !part.isEmpty {
            if let modifier = Modifier(rawValue: part.uppercased()) {
                modifiers.append(modifier)
            } else {
                key = part
            }
        }
        guard let key, Chord.keyCode(for: key) != nil else { return nil }
        self.init(modifiers: modifiers, key: key)
    }

    // MARK: Display

    /// The chord as a person would write it.
    ///
    /// `spaceLabel` is passed in from the message catalogue because the space
    /// bar is the one key in the supported set whose name is a word rather than
    /// what is printed on it. Letters, digits and function keys read the same in
    /// every language.
    public func display(spaceLabel: String) -> String {
        let glyphs = modifiers.map(\.glyph).joined()
        let name = key == "space" ? spaceLabel : key.uppercased()
        // No separator between the glyphs and the key: that is how macOS writes
        // a shortcut everywhere else, and a shortcut that reads differently here
        // than in the menu bar reads as a different shortcut.
        return glyphs + name
    }

    // MARK: Validation

    /// Why a chord cannot serve as push-to-talk.
    ///
    /// These exist so the recorder can say *why* a chord was refused at the
    /// moment it is pressed, rather than accepting it and reporting a failure
    /// once the tap declines to arm.
    public enum Problem: Sendable, Equatable {
        case needsModifier
        case unsupportedKey
    }

    public static func validate(modifiers: [Modifier], key: String?) -> Problem? {
        guard let key, keyCode(for: key) != nil else { return .unsupportedKey }
        // A bare key would be swallowed system-wide by the event tap, taking it
        // away from every text field on the machine. That is not a thing to let
        // somebody do by accident.
        if modifiers.isEmpty { return .needsModifier }
        return nil
    }

    // MARK: Key codes

    /// The virtual key code an event tap will report, or `nil` for a key this
    /// app refuses to bind.
    ///
    /// The refusals are the deliberate part. Escape cancels the recorder, and
    /// Tab, Return, Delete and the arrows are what every dialog and text field
    /// on the system is built out of; a global tap on one of those takes it away
    /// everywhere.
    public static func keyCode(for key: String) -> Int64? {
        if key == "space" { return 49 }
        if let function = functionKeyCodes[key.uppercased()] { return function }
        guard key.count == 1, let scalar = key.unicodeScalars.first else { return nil }
        if scalar.properties.isLowercase || CharacterSet.letters.contains(scalar) {
            return letterKeyCodes[Character(key.lowercased())]
        }
        if let digit = digitKeyCodes[Character(key)] { return digit }
        return nil
    }

    /// The key name for a virtual key code, for turning a keypress back into a
    /// chord in the recorder.
    public static func keyName(for code: Int64) -> String? {
        if code == 49 { return "space" }
        if let name = functionKeyCodes.first(where: { $0.value == code })?.key { return name }
        if let letter = letterKeyCodes.first(where: { $0.value == code })?.key { return String(letter) }
        if let digit = digitKeyCodes.first(where: { $0.value == code })?.key { return String(digit) }
        return nil
    }

    private static let letterKeyCodes: [Character: Int64] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9,
        "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17,
        "o": 31, "u": 32, "i": 34, "p": 35, "l": 37, "j": 38, "k": 40,
        "n": 45, "m": 46,
    ]

    private static let digitKeyCodes: [Character: Int64] = [
        "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23,
        "9": 25, "7": 26, "8": 28, "0": 29,
    ]

    /// F1–F20. F21 upward exist in the XKB namespace but not on any keyboard
    /// macOS reports, so binding one would produce a shortcut nobody can press.
    private static let functionKeyCodes: [String: Int64] = [
        "F1": 122, "F2": 120, "F3": 99, "F4": 118, "F5": 96, "F6": 97, "F7": 98,
        "F8": 100, "F9": 101, "F10": 109, "F11": 103, "F12": 111, "F13": 105,
        "F14": 107, "F15": 113, "F16": 106, "F17": 64, "F18": 79, "F19": 80, "F20": 90,
    ]
}
