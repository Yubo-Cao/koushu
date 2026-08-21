import AppKit
import SwiftUI

/// Screenshot rig.
///
/// Liquid Glass is a function of what is behind it, so a single screenshot on a
/// single background says nothing. These are our own windows placed behind the
/// bar — the user's own windows are never moved, resized or activated.
///
/// Ordered at `.floating`, i.e. above ordinary application windows and below the
/// bar's `.statusBar`, so the sandwich is always backdrop → bar.
enum BackdropKind: String, CaseIterable {
    case none
    case light      // bright wallpaper + a light document
    case terminal   // dark terminal, matching the Tauri reference shot
    case color      // saturated content
}

final class BackdropPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    init(frame: NSRect) {
        super.init(contentRect: frame,
                   styleMask: [.borderless, .nonactivatingPanel],
                   backing: .buffered, defer: false)
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        isFloatingPanel = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        ignoresMouseEvents = true
        hasShadow = false
        isOpaque = true
        backgroundColor = .black
    }
}

@MainActor
final class BackdropController {
    private var panel: BackdropPanel?
    private(set) var kind: BackdropKind = .none

    /// Where the backdrop sits relative to ordinary windows.
    ///
    /// Two positions, because there are two things to photograph. The bar is a
    /// `.statusBar` panel above everything, so its backdrop has to be above
    /// ordinary windows too. The main and settings windows *are* ordinary
    /// windows, and their sidebar and toolbar materials sample what is behind
    /// the window — so their backdrop has to be underneath, standing in for the
    /// desktop.
    private(set) var isBelowWindows = false

    static let size = NSSize(width: 1300, height: 600)
    /// Big enough to sit behind the whole main window rather than peeking out
    /// around it, which would photograph as a frame rather than as a background.
    static let fullSize = NSSize(width: 1700, height: 1100)

    func show(_ kind: BackdropKind, below: Bool = false) {
        self.kind = kind
        isBelowWindows = below
        guard kind != .none else { hide(); return }

        let screen = NSScreen.main ?? NSScreen.screens[0]
        let vf = screen.visibleFrame
        let size = below ? Self.fullSize : Self.size
        let frame = NSRect(
            x: vf.midX - size.width / 2,
            y: below ? vf.midY - size.height / 2 : vf.minY + 24,
            width: size.width,
            height: size.height
        )

        let p = panel ?? BackdropPanel(frame: frame)
        // `.normal - 1` rather than a desktop level: it has to be under our own
        // windows but still over the user's wallpaper, and it must not end up
        // under the user's windows where it would be invisible and pointless.
        p.level = below ? NSWindow.Level(rawValue: NSWindow.Level.normal.rawValue - 1) : .floating
        p.setFrame(frame, display: false)
        let host = NSHostingView(rootView: BackdropView(kind: kind))
        host.frame = NSRect(origin: .zero, size: frame.size)
        host.autoresizingMask = [.width, .height]
        p.contentView = host
        p.orderFrontRegardless()
        panel = p
    }

    func hide() {
        panel?.orderOut(nil)
    }

    func setAppearance(_ appearance: NSAppearance?) {
        panel?.appearance = appearance
    }
}

// MARK: - Backdrop content

struct BackdropView: View {
    let kind: BackdropKind

    var body: some View {
        switch kind {
        case .none: Color.clear
        case .light: LightBackdrop()
        case .terminal: TerminalBackdrop()
        case .color: ColorBackdrop()
        }
    }
}

/// Bright wallpaper with a light document on top of it.
///
/// Both halves matter: the gradient shows whether the material adapts to a high
/// key backdrop, and the document's text gives the refraction something with
/// hard edges to bend. A smooth gradient alone would hide a material that only
/// blurs.
private struct LightBackdrop: View {
    var body: some View {
        ZStack {
            MeshGradient(
                width: 3, height: 3,
                points: [
                    .init(0, 0), .init(0.5, 0), .init(1, 0),
                    .init(0, 0.5), .init(0.55, 0.45), .init(1, 0.5),
                    .init(0, 1), .init(0.45, 1), .init(1, 1)
                ],
                colors: [
                    Color(red: 1.00, green: 0.94, blue: 0.86),
                    Color(red: 0.99, green: 0.89, blue: 0.83),
                    Color(red: 0.96, green: 0.86, blue: 0.88),
                    Color(red: 0.94, green: 0.93, blue: 0.99),
                    Color(red: 0.99, green: 0.96, blue: 0.92),
                    Color(red: 0.87, green: 0.92, blue: 1.00),
                    Color(red: 0.98, green: 0.92, blue: 0.85),
                    Color(red: 0.92, green: 0.95, blue: 0.99),
                    Color(red: 0.85, green: 0.90, blue: 0.98)
                ]
            )

            // The card is tall on purpose. A backdrop whose content sits above
            // the bar tells you nothing: the glass ends up over blank gradient
            // and every material looks the same there. The interesting question
            // is what happens over text, so there has to be text underneath.
            VStack(alignment: .leading, spacing: 9) {
                Text("会议纪要 · 2026-08-08")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(Color(white: 0.12))
                ForEach(Array(Self.paragraph.enumerated()), id: \.offset) { _, line in
                    Text(line)
                        .font(.system(size: 14))
                        .foregroundStyle(Color(white: 0.28))
                }
            }
            .padding(34)
            .frame(width: 820, alignment: .leading)
            .frame(maxHeight: .infinity, alignment: .top)
            .background(Color.white.opacity(0.94))
            .clipShape(.rect(cornerRadius: 16, style: .continuous))
            .shadow(color: .black.opacity(0.12), radius: 24, y: 8)
            .padding(.vertical, 26)
        }
    }

    static let paragraph = [
        "决定把 macOS 端改写成原生 SwiftUI，Linux 与 Windows 继续沿用 Tauri。",
        "理由是结构性的，不是审美偏好：玻璃会响应滚动内容、在转场时形变、",
        "由系统实时合成，而 webview 对 AppKit 来说只是一个不透明矩形。",
        "先做悬浮语音条这一条，把最难、最不确定的部分打通再谈其它。",
        "共享核心走 UniFFI，流式接口用回调而不是轮询，取消必须进接口。",
        "存储先搬，因为它最自包含、测试最多，搬错了会立刻暴露出来。",
        "然后是 ASR 运行时和流式 worker，真正的复杂度都在那里。",
        "剩下 LLM、云端 ASR、许可证、试用计量，小而独立，最后再动。",
        "lib.rs 留成一层很薄的 Tauri shim；如果最后它不明显地薄，",
        "说明边界画错了地方，那就得回头重画，而不是继续往下堆。",
        "每一步 Tauri 版都必须还能跑。需要 flag day 的重构会被放弃。",
        "这份原型不回答要不要做，只回答原生能不能明显更好看。",
        "如果原生玻璃本身就对了，第二套 UI 买到的东西比它的代价少。",
        "核心抽取无论如何都是赚的，这一点和 SwiftUI 无关。"
    ]
}

/// Dark terminal — deliberately shaped like the Tauri reference screenshot so
/// the two can be compared without arguing about the background.
private struct TerminalBackdrop: View {
    var body: some View {
        ZStack {
            Color(red: 0.118, green: 0.125, blue: 0.153)
            VStack(alignment: .leading, spacing: 10) {
                ForEach(Array(Self.lines.enumerated()), id: \.offset) { _, line in
                    Text(line)
                        .font(.system(size: 17, weight: .medium, design: .monospaced))
                        .foregroundStyle(Color(white: 0.80))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .padding(40)
        }
    }

    /// Long lines on purpose, filling the width as well as the height. Short
    /// lines left the bar sitting beside a narrow column of text with nothing
    /// but flat colour behind it, which is the one background on which every
    /// material looks identical.
    static let lines = [
        "$ swift build -c release --disable-sandbox && ./build.sh   # 5.6s, arm64, macOS 27 SDK",
        "  designated => identifier \"dev.yubo.koushu\" and certificate leaf = H\"8062…\"",
        "",
        "// The whole product depends on one property: the bar must never take the keyboard.",
        "styleMask: [.borderless, .nonactivatingPanel]        // clickable without activating us",
        "override var canBecomeKey: Bool  { false }           // nothing inside can pull focus",
        "level = .statusBar                                   // above other apps' full screen",
        "collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]",
        "NSApp.setActivationPolicy(.accessory)                // no Dock tile, never frontmost",
        "panel.orderFrontRegardless()                         // show without a key-window request",
        "",
        "let g: Glass = .regular.interactive().tint(.red)     // tint the orb, never the panel",
        "content.glassEffect(g, in: .rect(cornerRadius: r, style: .continuous))",
        "        .glassEffectID(\"body\", in: namespace)        // morph, don't cross-fade",
        "",
        "[audio] engine started, format=<AVAudioFormat 0x600002a1c000:  1 ch, 48000 Hz, Float32>",
        "[hotkey] event tap armed on ⌃⌥Space                  // CGEventTap, .defaultTap",
        "[mic] authorization granted=true                     // survives rebuilds now",
        "",
        "# ad-hoc signing rotates the code identity every build and silently voids every TCC",
        "# grant; the checkbox stays ticked and stops working. A self-signed cert fixes it.",
        "$ codesign -d -r- FunASRBar.app | grep designated"
    ]
}

/// Saturated content: the case where a material that merely blurs turns into
/// grey mud, and a material that refracts keeps the colour moving.
private struct ColorBackdrop: View {
    var body: some View {
        ZStack {
            MeshGradient(
                width: 3, height: 3,
                points: [
                    .init(0, 0), .init(0.5, 0), .init(1, 0),
                    .init(0, 0.5), .init(0.5, 0.55), .init(1, 0.5),
                    .init(0, 1), .init(0.5, 1), .init(1, 1)
                ],
                colors: [
                    Color(red: 0.05, green: 0.24, blue: 0.85),
                    Color(red: 0.55, green: 0.13, blue: 0.85),
                    Color(red: 0.92, green: 0.16, blue: 0.44),
                    Color(red: 0.00, green: 0.62, blue: 0.85),
                    Color(red: 0.36, green: 0.20, blue: 0.78),
                    Color(red: 1.00, green: 0.42, blue: 0.15),
                    Color(red: 0.00, green: 0.78, blue: 0.60),
                    Color(red: 0.10, green: 0.55, blue: 0.90),
                    Color(red: 1.00, green: 0.78, blue: 0.10)
                ]
            )
            // Hard edges, so refraction has something to displace.
            HStack(spacing: 26) {
                ForEach(0..<9, id: \.self) { i in
                    Capsule()
                        .fill(.white.opacity(i.isMultiple(of: 2) ? 0.22 : 0.06))
                        .frame(width: 34)
                }
            }
            .rotationEffect(.degrees(18))
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
