import AppKit
import Foundation

/// A file the app polls for commands, and a file it writes state into.
///
/// This exists because the app is driven over SSH from another machine. The
/// hotkey path needs Accessibility and the panel has no clickable chrome, so
/// without a side channel there is no way to put the bar into a given state in
/// order to photograph it. It also lets the look be tuned live: on this machine
/// every rebuild rotates the ad-hoc signature and costs a TCC re-grant, so
/// "rebuild to try a different corner radius" is expensive.
@MainActor
final class ControlChannel {
    static let commandPath = NSHomeDirectory() + "/.funasr-bar-cmd"
    static let statusPath = NSHomeDirectory() + "/.funasr-bar-status"

    private var timer: Timer?
    var handler: ((String) -> Void)?
    var statusProvider: (() -> [String: Any])?

    func start() {
        try? "".write(toFile: Self.commandPath, atomically: true, encoding: .utf8)
        timer = Timer.scheduledTimer(withTimeInterval: 0.06, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    private var statusCounter = 0

    private func tick() {
        if let s = try? String(contentsOfFile: Self.commandPath, encoding: .utf8),
           !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            // Consume before dispatching so the same line never fires twice.
            try? "".write(toFile: Self.commandPath, atomically: true, encoding: .utf8)
            for line in s.split(separator: "\n") {
                let cmd = line.trimmingCharacters(in: .whitespaces)
                if !cmd.isEmpty { handler?(cmd) }
            }
        }
        // ~5 Hz is plenty for a status file nobody watches in real time.
        statusCounter += 1
        if statusCounter % 3 == 0, let provider = statusProvider {
            let dict = provider()
            if let data = try? JSONSerialization.data(withJSONObject: dict, options: [.prettyPrinted, .sortedKeys]) {
                try? data.write(to: URL(fileURLWithPath: Self.statusPath))
            }
        }
    }
}
