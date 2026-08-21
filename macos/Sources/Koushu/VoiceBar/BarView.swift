import KoushuCore
import SwiftUI

/// The floating voice bar.
///
/// Deliberately absent: any highlight, sheen, stroke or gradient of our own. The
/// Tauri version failed exactly there — CSS painted a specular highlight on top
/// of the native material and the result read as over-lit and stiff. Liquid
/// Glass already lights its own edges, thickens its own rim and darkens itself
/// against dark backdrops. Anything we add on top fights the system compositor
/// and wins, which is the problem.
///
/// This is also the one thing in the app a webview could not have done at any
/// level of effort: the orb and the panel are two separate glass bodies with
/// their own shapes and their own responses, merging and separating. To AppKit a
/// webview is a single opaque rectangle, so it can sit on one sheet of glass but
/// its contents cannot each be glass.
struct BarView: View {
    @Bindable var app: AppModel
    @Bindable var bar: VoiceBarModel
    @Namespace private var glass

    private var hasText: Bool {
        !displayText.isEmpty
    }

    /// What the panel is saying right now.
    ///
    /// The live partial while recording, the committed transcript once it
    /// lands. Both are the same surface because they are the same thing at two
    /// stages of certainty, and swapping between two surfaces would make the
    /// commit read as a new event rather than as the answer settling.
    private var displayText: String {
        switch app.activity {
        case .recording: app.partial
        case .transcribing: app.partial
        case .idle: app.lastTranscript
        }
    }

    /// The long panel stays neutral in every state.
    ///
    /// Tinting it while recording was the obvious idea and it was wrong twice:
    /// over a large area the system's tint is too weak to read as status at all,
    /// and colouring the surface that carries the transcript is exactly the
    /// "colour on the translucent layer" mistake — the text has to stay legible
    /// over whatever is behind it.
    private var panelGlass: Glass {
        var glass: Glass = bar.clearStyle ? .clear : .regular
        if bar.interactiveGlass { glass = glass.interactive() }
        return glass
    }

    /// Status lives on the orb instead: a small area, so the same tint reads as
    /// a colour rather than a wash, and it is the control the user pressed.
    private var orbGlass: Glass {
        var glass: Glass = bar.clearStyle ? .clear : .regular
        if bar.interactiveGlass { glass = glass.interactive() }
        if bar.tintWhileRecording && app.activity == .recording {
            glass = glass.tint(.red)
        }
        return glass
    }

    var body: some View {
        GlassEffectContainer(spacing: bar.glassSpacing) {
            HStack(alignment: .center, spacing: bar.stackSpacing) {
                orb
                bodyPanel
            }
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { rect in
                bar.barRectInView = rect
            }
        }
        // Bottom-anchored so the bar grows up and out from a fixed point instead
        // of drifting: enter and exit share one path.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        .padding(.bottom, 28)
    }

    // MARK: Orb

    private var orb: some View {
        Image(systemName: orbSymbol)
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(orbForeground)
            .frame(width: bar.orbSize, height: bar.orbSize)
            .glassEffect(orbGlass, in: .circle)
            .glassEffectID("orb", in: glass)
    }

    /// Reflects the microphone, and only the microphone.
    ///
    /// It used to show a slashed mic whenever the *hotkey* was unavailable,
    /// which is a mapping error: a control must report the thing it controls. A
    /// missing Accessibility grant does not break the microphone, and saying it
    /// does sends the user to fix the wrong setting.
    private var orbSymbol: String {
        app.microphoneGranted == false ? "mic.slash.fill" : "mic.fill"
    }

    /// Recording status is carried by the glyph, not by the material.
    ///
    /// `Glass.tint(.red)` turned out to be nearly invisible even on an area as
    /// small as the orb — the system tint is a wash, not a fill, which is right
    /// for a material and useless for a state indicator. A solid colour on the
    /// foreground element reads instantly, and is what "put colour on a solid
    /// layer, not the translucent one" means in practice.
    private var orbForeground: some ShapeStyle {
        app.activity == .recording
            ? AnyShapeStyle(Color.red)
            : AnyShapeStyle(.primary)
    }

    // MARK: Body panel

    private var bodyPanel: some View {
        ZStack {
            if hasText {
                textContent.transition(.blurReplace)
            } else if app.activity == .recording || app.activity == .transcribing {
                recordingContent.transition(.blurReplace)
            } else {
                hintContent.transition(.blurReplace)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .frame(width: bar.bodyWidth(for: app.activity, hasText: hasText))
        .frame(minHeight: bar.orbSize)
        .glassEffect(
            panelGlass,
            in: .rect(cornerRadius: bar.bodyCornerRadius(hasText: hasText), style: .continuous)
        )
        .glassEffectID("body", in: glass)
    }

    // MARK: Contents

    private var hintContent: some View {
        Text(hintText)
            .font(.system(size: 12.5, weight: .medium))
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .frame(maxWidth: .infinity)
    }

    /// The bar says what is wrong with itself, not what it wishes were true.
    ///
    /// A bar that shows the chord while nothing is listening for it is the worst
    /// failure mode this app has: the user holds the key, nothing happens, and
    /// there is no way to tell whether they mis-pressed it or the app is broken.
    private var hintText: String {
        if !app.hotkeyArmed { return app.l(.hotkeyNotBound) }
        return app.chord.display(spaceLabel: app.l(.hotkeyKeySpace))
    }

    private var recordingContent: some View {
        HStack(spacing: 10) {
            // `.primary`, not white: over glass this is a vibrant style, so it
            // inverts correctly in light appearance and keeps contrast against
            // whatever is moving behind the bar.
            WaveformView(levels: app.levels)
                .foregroundStyle(.primary)
                .frame(height: 22)
            Text(Format.elapsed(app.elapsed))
                .font(.system(size: 12, weight: .medium).monospacedDigit())
                .foregroundStyle(.secondary)
        }
    }

    private var textContent: some View {
        // Slightly heavier than body weight: over a translucent surface a
        // regular weight in a flat grey loses its edges against whatever moves
        // behind it. Weight buys legibility without buying space.
        Text(displayText)
            .font(.system(size: 13.5, weight: .medium))
            .foregroundStyle(.primary)
            .lineSpacing(2.5)
            .lineLimit(3)
            .multilineTextAlignment(.leading)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Mirrored bar meter. Newest sample on the right.
///
/// No implicit animation: the heights *are* the audio. Anything that smooths
/// them on the way to the screen is latency, and latency is the one thing that
/// makes a level meter read as decoration rather than instrumentation.
struct WaveformView: View {
    let levels: [CGFloat]

    var body: some View {
        GeometryReader { geo in
            let count = max(levels.count, 1)
            // Fixed narrow bars, with the spacing derived from the width rather
            // than the other way round. Deriving the *width* from the space
            // available made the bars nearly as wide as they were tall, and a
            // column of roughly square marks reads as a row of dots no matter
            // what the audio is doing.
            let width: CGFloat = 3
            let spacing = count > 1
                ? max(2, (geo.size.width - width * CGFloat(count)) / CGFloat(count - 1))
                : 0
            HStack(alignment: .center, spacing: spacing) {
                ForEach(0..<count, id: \.self) { index in
                    // Floor is a dash, not a dot: clamping the height to the bar
                    // width turns silence into a row of circles, which reads as
                    // a broken control rather than a quiet room.
                    Capsule(style: .continuous)
                        .frame(width: width, height: max(2.5, levels[index] * geo.size.height))
                }
            }
            .frame(width: geo.size.width, height: geo.size.height, alignment: .center)
        }
        .transaction { $0.animation = nil }
    }
}
