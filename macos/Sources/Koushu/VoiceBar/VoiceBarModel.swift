import Observation
import SwiftUI

/// Geometry and look of the floating bar.
///
/// Separate from ``AppModel`` because none of it is application state: these are
/// the numbers that decide how the bar is drawn, live-editable through the
/// control channel so the look can be judged without a rebuild. On a locally
/// signed build every rebuild is a different program to macOS, so "rebuild to
/// try a corner radius" costs a permission round trip.
@MainActor
@Observable
final class VoiceBarModel {
    /// The bar's real rect inside the hosting view, reported by SwiftUI.
    /// Drives click-through and tells the screenshot rig what to crop.
    var barRectInView: CGRect?

    // MARK: Tunables
    //
    // `glassSpacing` is 0 deliberately. Raising it makes the container merge the
    // orb and the panel into one blob, and at rest that reads as a mistake — a
    // pinched waist that looks like two things failed to separate rather than
    // one thing that was designed. Apple's own glass merges look like that only
    // *during* a transition. Two clean shapes, one round and one long, say what
    // they are: a button, and the thing the button fills.
    var glassSpacing: CGFloat = 0
    var stackSpacing: CGFloat = 8
    var orbSize: CGFloat = 40
    var clearStyle = false
    var tintWhileRecording = true
    var interactiveGlass = true

    // MARK: Derived geometry

    func bodyWidth(for activity: Activity, hasText: Bool) -> CGFloat {
        if hasText { return 404 }
        switch activity {
        case .idle: return 132
        case .recording: return 268
        case .transcribing: return 268
        }
    }

    func bodyCornerRadius(hasText: Bool) -> CGFloat {
        hasText ? 22 : 20
    }
}
