import AppKit
import AVFoundation
import ApplicationServices
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = BarModel()
    private var controller: BarWindowController!
    private var audio: AudioLevelMonitor!
    private let hotkey = HotkeyTap()
    private let control = ControlChannel()
    private let backdrop = BackdropController()

    // Synthetic levels, only for tuning the look before the microphone grant
    // exists. Any screenshot taken with this on is a picture of a fake meter and
    // has to be labelled as one.
    private var demoTimer: Timer?
    private var demoPhase: Double = 0

    private var statusItem: NSStatusItem?

    /// Everything intrusive is opt-in.
    ///
    /// This runs on the author's working machine, so the safe default is a
    /// process that launches, shows nothing, grabs no keys, and waits. A bare
    /// launch must be indistinguishable from not launching it at all.
    private lazy var args = Set(CommandLine.arguments.dropFirst())

    func applicationDidFinishLaunching(_ n: Notification) {
        NSApp.setActivationPolicy(.accessory)

        audio = AudioLevelMonitor(model: model)
        controller = BarWindowController(model: model)

        // --hotkey only. A global CGEventTap swallows ⌃⌥Space system-wide; arming
        // it by default would take keys out from under whatever the user is
        // typing into, for a prototype they did not ask to have running.
        if args.contains("--hotkey") {
            hotkey.onPress = { [weak self] in self?.beginUtterance() }
            hotkey.onRelease = { [weak self] in self?.endUtterance() }
            model.hotkeyArmed = hotkey.start()
        } else {
            model.hotkeyArmed = false
        }

        if args.contains("--menubar") { installStatusItem() }

        // Asking for the microphone puts a system dialog on screen, so it is
        // only requested when something is actually going to record.
        if args.contains("--mic") || args.contains("--hotkey") {
            audio.requestAccess { [weak self] granted in
                guard let self else { return }
                model.micAuthorized = granted
                model.micDenied = !granted
                NSLog("[mic] authorization granted=\(granted)")
            }
        }

        control.handler = { [weak self] cmd in self?.run(cmd) }
        control.statusProvider = { [weak self] in self?.status() ?? [:] }
        control.start()

        if args.contains("--show") { controller.show() }

        NSLog("[bar] up, hidden=\(!controller.isVisible) hotkey=\(model.hotkeyArmed) pid=\(getpid())")
    }

    /// `NSStatusItem` — a menu-bar extra, which a webview cannot be. The menu is
    /// a real `NSMenu`: it renders in the system's own process, so it gets the
    /// platform's materials, keyboard driving and accessibility for free.
    private func installStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "FunASR")
        let menu = NSMenu()
        menu.addItem(withTitle: "显示语音条", action: #selector(menuShow), keyEquivalent: "").target = self
        menu.addItem(withTitle: "隐藏", action: #selector(menuHide), keyEquivalent: "").target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: "退出", action: #selector(menuQuit), keyEquivalent: "q").target = self
        item.menu = menu
        statusItem = item
    }

    @objc private func menuShow() { controller.show() }
    @objc private func menuHide() { controller.hide() }
    @objc private func menuQuit() { NSApp.terminate(nil) }

    // MARK: Utterance

    private func beginUtterance() {
        model.startRecording()
        audio.start()
    }

    private func endUtterance() {
        audio.stop()
        model.stopRecording()
    }

    // MARK: Status

    private func status() -> [String: Any] {
        // Evidence for the one property the product depends on: the bar is on
        // screen and the keyboard still belongs to somebody else.
        let front = NSWorkspace.shared.frontmostApplication
        var d: [String: Any] = [
            "phase": model.phase.rawValue,
            "accessibility": hotkey.isTrusted,
            "hotkeyArmed": model.hotkeyArmed,
            "focus": [
                "weAreActive": NSApp.isActive,
                "panelIsKey": controller.panel.isKeyWindow,
                "panelCanBecomeKey": controller.panel.canBecomeKey,
                "panelIsVisible": controller.panel.isVisible,
                "frontmostApp": front?.bundleIdentifier ?? front?.localizedName ?? "?",
                "frontmostPid": front?.processIdentifier ?? -1,
                "ourPid": getpid(),
                "activationPolicy": NSApp.activationPolicy() == .accessory ? "accessory" : "other"
            ],
            "micAuthorized": model.micAuthorized,
            "micStatus": String(describing: AVCaptureDevice.authorizationStatus(for: .audio).rawValue),
            "level": Double(model.level),
            "levelPeakRecent": Double(model.levels.max() ?? 0),
            "backdrop": backdrop.kind.rawValue,
            "appearance": NSApp.effectiveAppearance.name.rawValue,
            "demoLevels": demoTimer != nil
        ]
        if let r = controller.barScreenRect {
            let primary = NSScreen.screens.first?.frame ?? .zero
            d["barScreen"] = ["x": r.minX, "y": r.minY, "w": r.width, "h": r.height]
            // screencapture -R wants top-left origin, y growing down.
            d["barCapture"] = [
                "x": Int(r.minX.rounded()),
                "y": Int((primary.height - r.maxY).rounded()),
                "w": Int(r.width.rounded()),
                "h": Int(r.height.rounded())
            ]
        }
        return d
    }

    // MARK: Commands

    private func run(_ cmd: String) {
        let parts = cmd.split(separator: " ", maxSplits: 1).map(String.init)
        let verb = parts.first ?? ""
        let arg = parts.count > 1 ? parts[1] : ""
        NSLog("[cmd] \(cmd)")

        switch verb {
        case "show":   controller.show()
        case "hide":   controller.hide()
        case "idle":   model.goIdle()
        case "record": beginUtterance()
        case "stop":   endUtterance()

        case "inject":
            // Never automatic. Typing into whoever has focus is exactly the
            // thing you do not do by accident on somebody's real desktop.
            NSLog("[inject] target=\(TextInjector.focusTargetDescription)")
            TextInjector.insert(arg.isEmpty ? model.transcript : arg)
        case "text":
            audio.stop()
            model.show(text: arg.isEmpty ? BarModel.stubTranscripts[0] : arg)

        case "backdrop":
            backdrop.show(BackdropKind(rawValue: arg) ?? .none)

        case "appearance":
            switch arg {
            case "dark":
                controller.setAppearance(.darkAqua)
                backdrop.setAppearance(NSAppearance(named: .darkAqua))
            case "light":
                controller.setAppearance(.aqua)
                backdrop.setAppearance(NSAppearance(named: .aqua))
            default:
                controller.setAppearance(nil)
                backdrop.setAppearance(nil)
            }

        case "pos":
            controller.reposition(bottomInset: CGFloat(Double(arg) ?? 96))

        case "ax-prompt":
            // Opens the standard "grant Accessibility" alert on the GUI.
            _ = AXIsProcessTrustedWithOptions(
                [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary)

        case "ax-recheck":
            hotkey.refreshTrust()
            model.hotkeyArmed = hotkey.start()

        case "mic-recheck":
            audio.requestAccess { [weak self] ok in
                self?.model.micAuthorized = ok
                self?.model.micDenied = !ok
            }

        case "demo":
            arg == "on" ? startDemoLevels() : stopDemoLevels()

        case "set":
            applyTunable(arg)

        case "quit":
            NSApp.terminate(nil)

        default:
            NSLog("[cmd] unknown: \(cmd)")
        }
    }

    private func applyTunable(_ arg: String) {
        let kv = arg.split(separator: " ").map(String.init)
        guard kv.count == 2 else { return }
        let v = Double(kv[1]) ?? 0
        withAnimation(Motion.expand) {
            switch kv[0] {
            case "glassSpacing":  model.glassSpacing = CGFloat(v)
            case "stackSpacing":  model.stackSpacing = CGFloat(v)
            case "orbSize":       model.orbSize = CGFloat(v)
            case "clear":         model.clearStyle = v != 0
            case "tint":          model.tintWhileRecording = v != 0
            case "interactive":   model.interactiveGlass = v != 0
            default: break
            }
        }
    }

    // MARK: Demo levels

    private func startDemoLevels() {
        guard demoTimer == nil else { return }
        model.startRecording()
        demoTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.demoPhase += 0.06
                let a = 0.45 + 0.35 * sin(self.demoPhase * 1.7)
                let b = 0.25 * sin(self.demoPhase * 5.3 + 1.1)
                let c = Double.random(in: -0.06...0.06)
                self.model.push(level: CGFloat(max(0.02, min(1, a + b + c))))
                self.model.elapsed += 1.0 / 60.0
            }
        }
    }

    private func stopDemoLevels() {
        demoTimer?.invalidate()
        demoTimer = nil
    }
}
