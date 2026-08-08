//! Anchor the voice bar to a screen edge as a proper desktop panel.
//!
//! An always-on-top window positioned by hand is not the same thing. A real
//! panel sits above normal windows without joining the window stack, stays out
//! of the taskbar and alt-tab, follows the screen edge when the resolution
//! changes, and — on Wayland — can reserve space so maximised windows do not
//! sit underneath it.
//!
//! | Platform | Mechanism |
//! |---|---|
//! | Linux/Wayland | `wlr-layer-shell` via gtk-layer-shell |
//! | macOS | `NSPanel` at status-bar level, joining all Spaces |

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
mod blur;
#[cfg(target_os = "linux")]
mod linux_layer;
#[cfg(target_os = "macos")]
mod macos_panel;

/// Where the bar sits. Centre variants hug an edge midpoint, which is what
/// most dictation bars use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl PanelAnchor {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "top-left" => Self::TopLeft,
            "top-center" => Self::TopCenter,
            "top-right" => Self::TopRight,
            "bottom-left" => Self::BottomLeft,
            "bottom-center" => Self::BottomCenter,
            "bottom-right" => Self::BottomRight,
            _ => return None,
        })
    }

    pub fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }

    /// Horizontal side, or `None` for the centred variants.
    pub fn horizontal(self) -> Option<bool> {
        match self {
            Self::TopLeft | Self::BottomLeft => Some(true),
            Self::TopRight | Self::BottomRight => Some(false),
            _ => None,
        }
    }
}

/// What the platform actually managed to do. `true` for `layer_shell` means a
/// genuine panel; `false` means it fell back to a positioned always-on-top
/// window, which still works but can be occluded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelStatus {
    pub anchored: bool,
    pub layer_shell: bool,
    pub detail: String,
}

/// Anchor the given window.
///
/// `draggable` picks between two genuinely different window kinds, and the
/// choice is a real trade-off rather than an implementation detail:
///
/// - `false` uses wlr-layer-shell on Wayland: a true overlay panel that no
///   window can cover, but one the compositor positions from its anchor. Such
///   a surface has no toplevel-move request, so it can never follow a cursor.
/// - `true` uses an ordinary always-on-top window, which can be dragged
///   natively and snapped to an edge afterwards, at the cost of being
///   occludable by fullscreen windows.
///
/// Dragging is the more valuable of the two for a bar the user is expected to
/// reposition, so it is the default.
pub fn anchor(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
    draggable: bool,
) -> Result<PanelStatus, String> {
    if draggable {
        return fallback_position(window, anchor, margin);
    }

    #[cfg(target_os = "linux")]
    {
        match linux_layer::anchor(window, anchor, margin) {
            Ok(status) => return Ok(status),
            Err(err) => {
                let status = fallback_position(window, anchor, margin)?;
                return Ok(PanelStatus {
                    detail: format!("{} (layer-shell unavailable: {err})", status.detail),
                    ..status
                });
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        match macos_panel::anchor(window, anchor, margin) {
            Ok(status) => return Ok(status),
            Err(err) => {
                let status = fallback_position(window, anchor, margin)?;
                return Ok(PanelStatus {
                    detail: format!("{} (NSPanel setup failed: {err})", status.detail),
                    ..status
                });
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fallback_position(window, anchor, margin)
}

/// Position the window against a screen corner using plain window geometry.
///
/// `size_override` exists for the resize path. `set_size` does not take effect
/// synchronously, so reading `outer_size()` right after it returns the *old*
/// size and the window gets placed as if it were still its previous shape —
/// which, on a HiDPI display, is off by the scale factor as well. Callers that
/// just resized pass the size they asked for instead of racing the compositor.
pub fn fallback_position(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
) -> Result<PanelStatus, String> {
    fallback_position_sized(window, anchor, margin, None)
}

/// Ask the compositor to blur the desktop behind a window.
///
/// Best-effort: returns the reason on failure so it can be reported once,
/// never retried in a loop.
#[cfg(target_os = "linux")]
pub fn enable_background_blur(window: &tauri::WebviewWindow) -> Result<(), String> {
    blur::enable_for(window)
}

#[cfg(not(target_os = "linux"))]
pub fn enable_background_blur(_window: &tauri::WebviewWindow) -> Result<(), String> {
    // macOS gets this from the NSVisualEffectView side instead.
    Ok(())
}

/// Logical geometry of the output the window is on, plus the window's own
/// logical size. Everything drag-related works in this space.
pub struct OutputGeometry {
    pub width: f64,
    pub height: f64,
    pub win_width: f64,
    pub win_height: f64,
}

pub fn output_geometry(window: &tauri::WebviewWindow) -> Result<OutputGeometry, String> {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|list| list.into_iter().next())
        })
        .ok_or_else(|| "No monitor found for the voice bar.".to_string())?;
    let scale = monitor.scale_factor();
    let size = window.outer_size().map_err(|err| err.to_string())?;
    Ok(OutputGeometry {
        width: monitor.size().width as f64 / scale,
        height: monitor.size().height as f64 / scale,
        win_width: size.width as f64 / scale,
        win_height: size.height as f64 / scale,
    })
}

/// Re-apply an anchor, preferring the platform panel mechanism.
pub fn reposition(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
    size_override: Option<tauri::PhysicalSize<u32>>,
) -> Result<PanelStatus, String> {
    #[cfg(target_os = "linux")]
    if let Ok(status) = linux_layer::anchor(window, anchor, margin) {
        return Ok(status);
    }
    fallback_position_sized(window, anchor, margin, size_override)
}

/// Move a layer surface to an absolute logical position (Linux/Wayland only).
#[cfg(target_os = "linux")]
pub fn move_to(window: &tauri::WebviewWindow, x: i32, y: i32) -> Result<(), String> {
    linux_layer::move_to(window, x, y)
}

#[cfg(not(target_os = "linux"))]
pub fn move_to(window: &tauri::WebviewWindow, x: i32, y: i32) -> Result<(), String> {
    window
        .set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

pub fn fallback_position_sized(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
    size_override: Option<tauri::PhysicalSize<u32>>,
) -> Result<PanelStatus, String> {
    // An unmapped window has no current monitor yet — this runs at startup,
    // before the bar has ever been shown — so fall back through the primary
    // display and finally any known display rather than failing outright.
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|list| list.into_iter().next())
        })
        .ok_or_else(|| "No monitor found for the voice bar.".to_string())?;

    // Everything is computed in *logical* pixels and applied with
    // LogicalPosition.
    //
    // Mixing units is what put the bar in the middle of the screen. Monitor
    // position and size are physical, window geometry as the compositor
    // reports it is logical, and on a mixed-DPI desk (a 1.0 external above a
    // 1.25 laptop panel) the two spaces do not even share an origin, so
    // `origin + size - height` silently lands hundreds of pixels off.
    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;
    let origin_x = monitor.position().x as f64 / scale;
    let origin_y = monitor.position().y as f64 / scale;

    let physical = match size_override {
        Some(size) => size,
        None => window.outer_size().map_err(|err| err.to_string())?,
    };
    let win_w = physical.width as f64 / scale;
    let win_h = physical.height as f64 / scale;

    // Margin stays in logical pixels: the same visual inset on every display.
    let margin = margin as f64;

    let x = match anchor.horizontal() {
        Some(true) => origin_x + margin,
        Some(false) => origin_x + screen_w - win_w - margin,
        None => origin_x + (screen_w - win_w) / 2.0,
    };
    let y = if anchor.is_top() {
        origin_y + margin
    } else {
        origin_y + screen_h - win_h - margin
    };

    window
        .set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())?;

    Ok(PanelStatus {
        anchored: true,
        layer_shell: false,
        detail: "Positioned as an always-on-top window. It can be covered by fullscreen windows."
            .to_string(),
    })
}
