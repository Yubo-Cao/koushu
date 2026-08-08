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
use objc::{msg_send, sel, sel_impl};

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
