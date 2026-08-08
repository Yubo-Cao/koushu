//! Anchor the voice bar as a floating macOS panel.
//!
//! macOS has no layer-shell, but `NSWindow` exposes the same properties
//! piecemeal: a status-bar window level puts it above ordinary windows, and
//! the right collection behaviour keeps it present on every Space and out of
//! the window cycle.
//!
//! Focus is the important part. `NSNonactivatingPanelMask` lets the window
//! show without activating the app, which is what makes "hold the hotkey while
//! another app is focused, then paste back into it" work at all.

use objc::runtime::{Object, YES};
use objc::{class, msg_send, sel, sel_impl};

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

unsafe impl objc::Encode for NSPoint {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
    }
}

unsafe impl objc::Encode for NSSize {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGSize=dd}") }
    }
}

unsafe impl objc::Encode for NSRect {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

use super::{PanelAnchor, PanelStatus};

/// `NSStatusWindowLevel`. Above normal and floating windows, below the screen
/// saver and alerts.
const NS_STATUS_WINDOW_LEVEL: i64 = 25;

// NSWindowCollectionBehavior bits.
const CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
const TRANSIENT: u64 = 1 << 3;
const IGNORES_CYCLE: u64 = 1 << 6;
const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

/// NSWindowStyleMask bit that lets a panel show without activating the app.
const NONACTIVATING_PANEL: u64 = 1 << 7;

/// Install a real Liquid Glass backing view behind the webview.
///
/// macOS 26 added `NSGlassEffectView`; earlier versions get `NSVisualEffectView`
/// with `.behindWindow` blending, which frosts the desktop rather than the
/// window's own content. Both are **public API**, which matters beyond taste:
/// Tauri's `transparent` window relies on `macOSPrivateApi`, and shipping
/// private API bars the app from the Mac App Store. Doing the glass natively
/// removes that constraint and looks more like the system than any CSS
/// approximation could.
pub fn install_glass(window: &tauri::WebviewWindow) -> Result<&'static str, String> {
    let ns_window = window.ns_window().map_err(|err| err.to_string())? as *mut Object;
    if ns_window.is_null() {
        return Err("window has no NSWindow yet".to_string());
    }

    unsafe {
        // Public API: a transparent window without the private-API path.
        let _: () = msg_send![ns_window, setOpaque: false];
        let clear: *mut Object = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![ns_window, setBackgroundColor: clear];

        let content: *mut Object = msg_send![ns_window, contentView];
        if content.is_null() {
            return Err("window has no content view".to_string());
        }
        let bounds: NSRect = msg_send![content, bounds];

        // Prefer the real thing when the OS has it.
        let glass_class = objc::runtime::Class::get("NSGlassEffectView");
        let (view, kind): (*mut Object, &'static str) = if let Some(cls) = glass_class {
            let view: *mut Object = msg_send![cls, alloc];
            let view: *mut Object = msg_send![view, initWithFrame: bounds];
            (view, "NSGlassEffectView")
        } else {
            let cls = class!(NSVisualEffectView);
            let view: *mut Object = msg_send![cls, alloc];
            let view: *mut Object = msg_send![view, initWithFrame: bounds];
            // 0 = behindWindow: blur the desktop, not our own content.
            let _: () = msg_send![view, setBlendingMode: 0i64];
            // 1 = active: stay lit even when the app is not frontmost, which
            // matters for a bar that appears while another app has focus.
            let _: () = msg_send![view, setState: 1i64];
            (view, "NSVisualEffectView")
        };

        // Follow the window when it resizes; the bar changes size per state.
        let _: () = msg_send![view, setAutoresizingMask: 18u64]; // width | height
        // 0 = NSWindowBelow: sit behind the webview rather than over it.
        let _: () = msg_send![content, addSubview: view positioned: 0i64 relativeTo: std::ptr::null::<Object>()];
        Ok(kind)
    }
}

pub fn anchor(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
) -> Result<PanelStatus, String> {
    let ns_window = window.ns_window().map_err(|err| err.to_string())? as *mut Object;
    if ns_window.is_null() {
        return Err("window has no NSWindow yet".to_string());
    }

    unsafe {
        let _: () = msg_send![ns_window, setLevel: NS_STATUS_WINDOW_LEVEL];

        // Visible on every Space, skipped by Cmd-Tab and Mission Control
        // cycling, and allowed to sit over another app's fullscreen window.
        let behavior: u64 =
            CAN_JOIN_ALL_SPACES | TRANSIENT | IGNORES_CYCLE | FULL_SCREEN_AUXILIARY;
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];

        // Showing this window must not steal focus from whatever the user is
        // dictating into; without this the paste target changes mid-utterance.
        let mask: u64 = msg_send![ns_window, styleMask];
        let _: () = msg_send![ns_window, setStyleMask: mask | NONACTIVATING_PANEL];
        let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
        let _: () = msg_send![ns_window, setMovableByWindowBackground: YES];
    }

    // No layer-shell equivalent for edge anchoring, so place it geometrically.
    // Unlike the Linux fallback this is still a genuine panel: the level and
    // collection behaviour above are what matter, not how it got positioned.
    let status = super::fallback_position(window, anchor, margin)?;
    let _ = status;

    Ok(PanelStatus {
        anchored: true,
        layer_shell: false,
        detail: "Floating NSPanel at status-bar level, on all Spaces, never activating the app."
            .to_string(),
    })
}
