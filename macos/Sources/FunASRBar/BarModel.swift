import SwiftUI
import Observation

enum Phase: String, CaseIterable {
    case idle
    case recording
    case text
}

/// Motion constants.
///
/// Values follow the Apple fluid-interface convention of *damping ratio* +
/// *response* rather than mass/stiffness/damping. Response is not a duration —
/// a spring has no fixed duration, its settle time falls out of the parameters.
///
/// Expand carries a hint of overshoot (0.82) because a key press is an impulse
/// and the material should read as springing open. Collapse is near-critically
/// damped (0.92) because releasing a key is the *end* of an impulse; bounce
/// there would read as the UI having an opinion of its own.
enum Motion {
    static let expand = Animation.spring(response: 0.38, dampingFraction: 0.82)
    static let collapse = Animation.spring(response: 0.32, dampingFraction: 0.92)
    /// Content swap inside a panel whose size is already moving.
    static let content = Animation.spring(response: 0.30, dampingFraction: 1.0)
}

/// Everything the bar needs to draw itself. Tunables are live-editable through
/// the control file so the look can be A/B'd without a rebuild — on this
/// machine every rebuild changes the ad-hoc signature and revokes TCC grants,
/// so "rebuild to try a number" costs a permission round-trip.
@Observable
final class BarModel {
    var phase: Phase = .idle

    // Live audio, 0...1 after envelope shaping.
    var level: CGFloat = 0
    /// Newest sample last.
    var levels: [CGFloat] = Array(repeating: 0, count: BarModel.barCount)
    static let barCount = 30

    var elapsed: TimeInterval = 0
    var transcript: String = ""

    /// The bar's real rect inside the hosting view, reported by SwiftUI.
    /// Drives click-through and tells the screenshot rig what to crop.
    var barRectInView: CGRect?

    // Honest status, surfaced in the UI rather than only in the log.
    var hotkeyArmed = false
    var micAuthorized = false
    var micDenied = false

    // MARK: Tunables (control file)
    //
    // glassSpacing is 0 deliberately. Raising it makes the container merge the
    // orb and the panel into one blob, and at rest that reads as a mistake — a
    // pinched waist that looks like two things failed to separate rather than
    // one thing that was designed. Apple's own glass merges look like that only
    // *during* a transition. Two clean shapes, one round and one long, say what
    // they are: a button, and the thing the button fills.
    var glassSpacing: CGFloat = 0       // GlassEffectContainer merge proximity
    var stackSpacing: CGFloat = 8       // gap between orb and body
    var orbSize: CGFloat = 40
    var clearStyle = false              // .clear instead of .regular
    var tintWhileRecording = true
    var interactiveGlass = true

    // MARK: Derived geometry

    var bodyWidth: CGFloat {
        switch phase {
        case .idle: return 132
        case .recording: return 268
        case .text: return 404
        }
    }

    var bodyCornerRadius: CGFloat {
        switch phase {
        case .idle, .recording: return 20
        case .text: return 22
        }
    }

    var elapsedText: String {
        let s = Int(elapsed)
        return String(format: "%d:%02d", s / 60, s % 60)
    }

    // MARK: Transitions

    func startRecording() {
        guard phase != .recording else { return }
        elapsed = 0
        levels = Array(repeating: 0, count: Self.barCount)
        withAnimation(Motion.expand) { phase = .recording }
    }

    /// Stub for the shared Rust core. The real thing streams partials and then
    /// commits; here a fixed delay stands in for the commit so the *shape* of
    /// the interaction (release -> brief wait -> text) is honest even though
    /// the text is not.
    func stopRecording() {
        guard phase == .recording else { return }
        let sentence = Self.stubTranscripts[Int.random(in: 0..<Self.stubTranscripts.count)]
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.42) { [weak self] in
            guard let self, self.phase == .recording else { return }
            self.transcript = sentence
            withAnimation(Motion.expand) { self.phase = .text }
            DispatchQueue.main.asyncAfter(deadline: .now() + 4.0) { [weak self] in
                guard let self, self.phase == .text else { return }
                withAnimation(Motion.collapse) { self.phase = .idle }
            }
        }
    }

    func goIdle() {
        withAnimation(Motion.collapse) { phase = .idle }
    }

    func show(text: String) {
        transcript = text
        withAnimation(Motion.expand) { phase = .text }
    }

    func push(level newLevel: CGFloat) {
        levels.removeFirst()
        levels.append(newLevel)
    }

    static let stubTranscripts = [
        "把这条悬浮语音条改写成原生 SwiftUI，先把最难的部分打通。",
        "麦克风电平是真的，转写是假的——这一版只回答玻璃能不能做好看。",
        "液态玻璃的意义全在它对背后内容的反应，单一背景根本看不出好坏。"
    ]
}
