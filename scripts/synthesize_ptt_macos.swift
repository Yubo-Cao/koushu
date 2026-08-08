import CoreGraphics
import Foundation
// Synthesize a real Ctrl+Alt+Space hold so the CGEventTap sees both edges.
let src = CGEventSource(stateID: .hidSystemState)
let mods: CGEventFlags = [.maskControl, .maskAlternate]
func post(_ code: CGKeyCode, _ down: Bool) {
    let e = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: down)!
    e.flags = mods
    e.post(tap: .cghidEventTap)
}
print("按下 Ctrl+Alt+Space")
post(59, true); post(58, true); post(49, true)
Thread.sleep(forTimeInterval: 1.5)
print("松开")
post(49, false); post(58, false); post(59, false)
