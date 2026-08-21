import AppKit

// Explicit AppKit entry rather than a SwiftUI `App`.
//
// This process has to be able to exist with no window, no Dock tile and no
// ability to become the active application, because that is what it does for
// almost all of its life — and the panel it does own must never take the
// keyboard away from whatever the user is dictating into. Hand-rolling the
// entry point is the shortest way to guarantee both; a scene graph that is
// present from launch is the wrong starting position for an app whose default
// state is "nothing on screen".
//
// The delegate is a global because `NSApplication.delegate` does not retain it.
let koushuDelegate = MainActor.assumeIsolated { AppDelegate() }

MainActor.assumeIsolated {
    let app = NSApplication.shared
    app.delegate = koushuDelegate
    app.setActivationPolicy(.accessory)
    app.run()
}
