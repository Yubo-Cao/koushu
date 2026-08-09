import SwiftUI

/// The bar.
///
/// Deliberately absent: any highlight, sheen, stroke or gradient of our own.
/// The Tauri version failed exactly there — CSS painted a specular highlight on
/// top of the native material and the result read as "光感太强、生硬". Liquid
/// Glass already lights its own edges, thickens its own rim, and darkens itself
/// against dark backdrops. Anything we add on top fights the system compositor
/// and wins, which is the problem.
struct BarView: View {
    @Bindable var model: BarModel
    @Namespace private var glass

    /// The long panel stays neutral in every state.
    ///
    /// Tinting it red while recording was the obvious idea and it was wrong
    /// twice: over a large area the system's tint is too weak to read as status
    /// at all, and colouring the surface that carries the transcript is exactly
    /// the "colour on the translucent layer" mistake — the text has to stay
    /// legible over whatever is behind it.
    private var panelGlass: Glass {
        var g: Glass = model.clearStyle ? .clear : .regular
        if model.interactiveGlass { g = g.interactive() }
        return g
    }

    /// Status lives on the orb instead: a small area, so the same tint reads as
    /// a colour rather than a wash, and it is the control the user pressed.
    private var orbGlass: Glass {
        var g: Glass = model.clearStyle ? .clear : .regular
        if model.interactiveGlass { g = g.interactive() }
        if model.tintWhileRecording && model.phase == .recording {
            g = g.tint(.red)
        }
        return g
    }

    var body: some View {
        GlassEffectContainer(spacing: model.glassSpacing) {
            HStack(alignment: .center, spacing: model.stackSpacing) {
                orb
                bodyPanel
            }
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { rect in
                model.barRectInView = rect
            }
        }
        // Bottom-anchored so the bar grows up and out from a fixed point
        // instead of drifting: enter and exit share one path.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        .padding(.bottom, 28)
    }

    // MARK: Orb

    private var orb: some View {
        Image(systemName: orbSymbol)
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(orbForeground)
            .frame(width: model.orbSize, height: model.orbSize)
            .glassEffect(orbGlass, in: .circle)
            .glassEffectID("orb", in: glass)
    }

    /// Reflects the microphone, and only the microphone.
    ///
    /// It used to show a slashed mic whenever the *hotkey* was unavailable,
    /// which is a mapping error: a control must report the thing it controls.
    /// A missing Accessibility grant does not break the microphone, and saying
    /// it does sends the user to fix the wrong setting.
    private var orbSymbol: String {
        model.micDenied ? "mic.slash.fill" : "mic.fill"
    }

    /// Recording status is carried by the glyph, not by the material.
    ///
    /// `Glass.tint(.red)` turned out to be nearly invisible even on an area as
    /// small as the orb — the system tint is a wash, not a fill, which is right
    /// for a material and useless for a state indicator. A solid colour on the
    /// foreground element reads instantly and is what "put colour on a solid
    /// layer, not the translucent one" means in practice.
    private var orbForeground: some ShapeStyle {
        model.phase == .recording
            ? AnyShapeStyle(Color.red)
            : AnyShapeStyle(.primary)
    }

    // MARK: Body panel

    private var bodyPanel: some View {
        ZStack {
            if model.phase == .idle {
                hintContent.transition(.blurReplace)
            }
            if model.phase == .recording {
                recordingContent.transition(.blurReplace)
            }
            if model.phase == .text {
                textContent.transition(.blurReplace)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .frame(width: model.bodyWidth)
        .frame(minHeight: model.orbSize)
        .glassEffect(panelGlass, in: .rect(cornerRadius: model.bodyCornerRadius, style: .continuous))
        .glassEffectID("body", in: glass)
    }

    // MARK: Contents

    private var hintContent: some View {
        Text(model.hotkeyArmed ? "⌃⌥ Space" : "需要辅助功能权限")
            .font(.system(size: 12.5, weight: .medium))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity)
    }

    private var recordingContent: some View {
        HStack(spacing: 10) {
            // .primary, not white: over glass this is a vibrant style, so it
            // inverts correctly in light appearance and keeps contrast against
            // whatever is moving behind the bar.
            WaveformView(levels: model.levels)
                .foregroundStyle(.primary)
                .frame(height: 22)
            Text(model.elapsedText)
                .font(.system(size: 12, weight: .medium).monospacedDigit())
                .foregroundStyle(.secondary)
        }
    }

    private var textContent: some View {
        // Slightly heavier than body weight: over a translucent surface a
        // regular weight in a flat gray loses its edges against whatever moves
        // behind it. Weight buys legibility without buying space.
        Text(model.transcript)
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
/// No implicit animation: the heights are the audio. Anything that smooths them
/// on the way to the screen is latency, and latency is the one thing that makes
/// a level meter read as decoration rather than instrumentation.
struct WaveformView: View {
    let levels: [CGFloat]

    var body: some View {
        GeometryReader { geo in
            let n = max(levels.count, 1)
            // Fixed narrow bars with the spacing derived from the width, rather
            // than the other way round. Deriving the *width* from the space
            // available made the bars nearly as wide as they were tall, and a
            // column of ~square marks reads as a row of dots no matter what the
            // audio is doing.
            let w: CGFloat = 3
            let spacing = n > 1
                ? max(2, (geo.size.width - w * CGFloat(n)) / CGFloat(n - 1))
                : 0
            HStack(alignment: .center, spacing: spacing) {
                ForEach(0..<n, id: \.self) { i in
                    // Floor is a dash, not a dot: clamping the height to the bar
                    // width turns silence into a row of circles, which reads as
                    // a broken control rather than a quiet room.
                    Capsule(style: .continuous)
                        .frame(width: w, height: max(2.5, levels[i] * geo.size.height))
                }
            }
            .frame(width: geo.size.width, height: geo.size.height, alignment: .center)
        }
        .transaction { $0.animation = nil }
    }
}
