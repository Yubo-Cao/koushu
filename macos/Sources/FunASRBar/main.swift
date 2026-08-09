import AppKit

// Explicit AppKit entry rather than a SwiftUI `App`: this process owns a single
// non-activating panel and must never create a normal window or become the
// active application, and hand-rolling the entry point is the shortest way to
// guarantee both.
//
// The delegate is a global because `NSApplication.delegate` does not retain it.
let barDelegate = MainActor.assumeIsolated { AppDelegate() }

MainActor.assumeIsolated {
    let app = NSApplication.shared
    app.delegate = barDelegate
    app.setActivationPolicy(.accessory)
    app.run()
}
