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

    /// The inverse of `parse`, so a dock can round-trip to the UI and back.
    pub fn name(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopCenter => "top-center",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomCenter => "bottom-center",
            Self::BottomRight => "bottom-right",
        }
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

#[cfg(target_os = "macos")]
pub fn enable_background_blur(window: &tauri::WebviewWindow) -> Result<(), String> {
    macos_panel::install_glass(window).map(|kind| {
        eprintln!("[voice-bar] glass backing: {kind}");
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn enable_background_blur(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

/// Reshape any native material we own after the window changed size.
///
/// Only macOS has anything to do here: the glass is a view in our own window,
/// and a capsule's corner radius is half its height, so a bar that grew from
/// the idle stub to a transcript strip needs the radius recomputed. On Linux
/// the blur belongs to the compositor and follows the surface by itself.
#[cfg(target_os = "macos")]
pub fn sync_material(window: &tauri::WebviewWindow) {
    macos_panel::sync_glass(window);
}

#[cfg(not(target_os = "macos"))]
pub fn sync_material(_window: &tauri::WebviewWindow) {}

/// A rectangle in the global logical desktop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Squared distance from this rectangle's centre — only ever compared, so
    /// the square root would be arithmetic nobody reads.
    fn centre_distance_sq(&self, x: f64, y: f64) -> f64 {
        let dx = x - (self.x + self.width / 2.0);
        let dy = y - (self.y + self.height / 2.0);
        dx * dx + dy * dy
    }
}

/// Decide which output a window dragged to `(x, y)` belongs to, and where on
/// that output it is allowed to sit.
///
/// Returns the index of the chosen output and the position clamped inside it.
///
/// Split out from the Wayland plumbing because it is the part that was wrong
/// and the part that can be tested: a desktop is not a rectangle. These two
/// outputs make an L — a 2560x1440 external at the origin with a 2048x1280
/// laptop panel below it and 512 to the right — so the area below-left of the
/// laptop panel belongs to no output at all. A window whose centre lands there
/// has to go *somewhere*, and the nearest output is the answer that keeps it
/// closest to where the user let go.
pub fn place(outputs: &[Rect], x: f64, y: f64, win_w: f64, win_h: f64) -> Option<(usize, f64, f64)> {
    let (centre_x, centre_y) = (x + win_w / 2.0, y + win_h / 2.0);
    let index = outputs
        .iter()
        .position(|output| output.contains(centre_x, centre_y))
        .or_else(|| {
            outputs
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.centre_distance_sq(centre_x, centre_y)
                        .total_cmp(&b.centre_distance_sq(centre_x, centre_y))
                })
                .map(|(index, _)| index)
        })?;
    let target = outputs[index];

    // Keep the whole window on screen. `max` guards the case where the window
    // is wider than the output it landed on, which would otherwise make the
    // clamp range run backwards and panic.
    let max_x = (target.x + target.width - win_w).max(target.x);
    let max_y = (target.y + target.height - win_h).max(target.y);
    Some((index, x.clamp(target.x, max_x), y.clamp(target.y, max_y)))
}

/// Logical geometry of the output the window is on, plus the window's own
/// logical size. Everything drag-related works in this space.
///
/// The origin is included because a second output does not start at zero, and
/// leaving it out is what confined the bar to one screen: an "x" that means
/// *offset within the current output* and an "x" that means *position on the
/// desktop* look identical until the desks differ, and then every drag that
/// crosses a screen edge is wrong by the neighbouring output's origin.
pub struct OutputGeometry {
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
    pub win_width: f64,
    pub win_height: f64,
}

pub fn output_geometry(window: &tauri::WebviewWindow) -> Result<OutputGeometry, String> {
    // Linux reads GDK directly rather than going through tao's synthetic
    // physical pixels; see `linux_layer::LogicalGeometry` for what makes them
    // synthetic and why it matters on a mixed-scale desk.
    #[cfg(target_os = "linux")]
    {
        let geometry = linux_layer::geometry(window)?;
        return Ok(OutputGeometry {
            origin_x: geometry.origin_x,
            origin_y: geometry.origin_y,
            width: geometry.width,
            height: geometry.height,
            win_width: geometry.win_width,
            win_height: geometry.win_height,
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
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

        // Two scale factors, deliberately. Monitor geometry is physical in the
        // monitor's scale and window geometry is physical in the window's, and
        // dividing one by the other is the mistake this function used to make.
        let monitor_scale = monitor.scale_factor();
        let window_scale = window.scale_factor().unwrap_or(monitor_scale);
        let size = window.outer_size().map_err(|err| err.to_string())?;
        Ok(OutputGeometry {
            origin_x: monitor.position().x as f64 / monitor_scale,
            origin_y: monitor.position().y as f64 / monitor_scale,
            width: monitor.size().width as f64 / monitor_scale,
            height: monitor.size().height as f64 / monitor_scale,
            win_width: size.width as f64 / window_scale,
            win_height: size.height as f64 / window_scale,
        })
    }
}

/// Re-apply an anchor, preferring the platform panel mechanism.
pub fn reposition(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
    size_override: Option<tauri::LogicalSize<f64>>,
) -> Result<PanelStatus, String> {
    #[cfg(target_os = "linux")]
    if let Ok(status) = linux_layer::anchor(window, anchor, margin) {
        return Ok(status);
    }
    fallback_position_sized(window, anchor, margin, size_override)
}

/// Move the bar to an absolute position on the desktop, in logical pixels.
///
/// Coordinates span every output, so `x` keeps counting past the right edge of
/// the first screen and into the second. Clamping happens here, against the
/// output the bar lands on, rather than in the caller: only this layer knows
/// which output that turned out to be.
#[cfg(target_os = "linux")]
pub fn move_to(window: &tauri::WebviewWindow, x: i32, y: i32) -> Result<(), String> {
    linux_layer::move_to(window, x, y)
}

#[cfg(not(target_os = "linux"))]
pub fn move_to(window: &tauri::WebviewWindow, x: i32, y: i32) -> Result<(), String> {
    // Same clamping rule as the Wayland path, from the same tested function.
    // Only one output is offered here because this platform reports the window's
    // current screen directly rather than making us choose one.
    let geometry = output_geometry(window)?;
    let output = Rect {
        x: geometry.origin_x,
        y: geometry.origin_y,
        width: geometry.width,
        height: geometry.height,
    };
    let (_, x, y) = place(
        &[output],
        x as f64,
        y as f64,
        geometry.win_width,
        geometry.win_height,
    )
    .ok_or_else(|| "No monitor found for the voice bar.".to_string())?;
    window
        .set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| err.to_string())
}

/// Where the bar is, in global logical pixels, or `None` if the platform will
/// not say.
///
/// Wayland will not say: it never reports a surface's position to its own
/// client, so on Linux this is only answerable for a layer surface docked to
/// the top-left, where the margins are the position.
pub fn current_position(window: &tauri::WebviewWindow) -> Option<(f64, f64)> {
    #[cfg(target_os = "linux")]
    {
        return linux_layer::current_position(window);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let scale = window.scale_factor().ok()?;
        let position = window.outer_position().ok()?;
        Some((position.x as f64 / scale, position.y as f64 / scale))
    }
}

pub fn fallback_position_sized(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
    size_override: Option<tauri::LogicalSize<f64>>,
) -> Result<PanelStatus, String> {
    // Everything below is *logical* pixels, start to finish, and is applied
    // with LogicalPosition. Mixing units is what put the bar in the middle of
    // the screen: on a mixed-DPI desk (a 1.0 external above a 1.25 laptop
    // panel) monitor space and window space do not share a scale, so
    // `origin + size - height` silently lands hundreds of pixels off.
    //
    // `output_geometry` already resolves an unmapped window — this runs at
    // startup, before the bar has ever been shown — down to the primary or
    // first known display rather than failing outright.
    let mut geometry = output_geometry(window)?;

    // `set_size` does not take effect synchronously, so a caller that just
    // resized would otherwise read back the *old* size here and place the
    // window as if it were still its previous shape. Such callers pass the
    // size they asked for instead of racing the compositor.
    if let Some(size) = size_override {
        geometry.win_width = size.width;
        geometry.win_height = size.height;
    }

    // Margin stays in logical pixels: the same visual inset on every display.
    let margin = margin as f64;

    let x = match anchor.horizontal() {
        Some(true) => geometry.origin_x + margin,
        Some(false) => geometry.origin_x + geometry.width - geometry.win_width - margin,
        None => geometry.origin_x + (geometry.width - geometry.win_width) / 2.0,
    };
    let y = if anchor.is_top() {
        geometry.origin_y + margin
    } else {
        geometry.origin_y + geometry.height - geometry.win_height - margin
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The desk this was debugged on, in GDK logical pixels.
    ///
    /// The numbers matter: the laptop panel is 2560x1600 hardware running at a
    /// compositor scale of 1.25, which is 2048x1280 logical, and it sits below
    /// and 512 to the right of the external display. Everything the old code
    /// got wrong, it got wrong because those two outputs do not share a scale
    /// and the second one does not start at the origin.
    const EXTERNAL: Rect = Rect { x: 0.0, y: 0.0, width: 2560.0, height: 1440.0 };
    const LAPTOP: Rect = Rect { x: 512.0, y: 1440.0, width: 2048.0, height: 1280.0 };

    fn desk() -> Vec<Rect> {
        vec![EXTERNAL, LAPTOP]
    }

    const BAR_W: f64 = 190.0;
    const BAR_H: f64 = 44.0;

    #[test]
    fn a_bar_in_the_middle_of_an_output_stays_where_it_was_put() {
        let (index, x, y) = place(&desk(), 1000.0, 700.0, BAR_W, BAR_H).unwrap();
        assert_eq!(index, 0);
        assert_eq!((x, y), (1000.0, 700.0));
    }

    #[test]
    fn dragging_past_the_bottom_of_the_external_lands_on_the_laptop() {
        // Centre at y = 1500, which is 60 logical pixels into the laptop panel.
        let (index, _, y) = place(&desk(), 1200.0, 1478.0, BAR_W, BAR_H).unwrap();
        assert_eq!(index, 1, "should have crossed onto the laptop panel");
        // Clamped to the laptop's own origin, not to zero: the whole point is
        // that the second output does not start at the top of the desktop.
        assert_eq!(y, 1478.0);
    }

    #[test]
    fn a_bar_is_clamped_into_the_output_it_landed_on() {
        // Far past the right edge of the laptop panel.
        let (index, x, y) = place(&desk(), 9000.0, 2000.0, BAR_W, BAR_H).unwrap();
        assert_eq!(index, 1);
        assert_eq!(x, 512.0 + 2048.0 - BAR_W);
        assert_eq!(y, 2000.0);
    }

    #[test]
    fn the_clamp_is_relative_to_the_output_not_the_desktop() {
        // Dragged off the *left* of the laptop panel. The laptop starts at
        // x = 512, so that is the floor — clamping to 0 would drop the bar into
        // the dead space beside it, where no output exists.
        let (index, x, _) = place(&desk(), 0.0, 2000.0, BAR_W, BAR_H).unwrap();
        assert_eq!(index, 1);
        assert_eq!(x, 512.0);
    }

    #[test]
    fn the_dead_corner_of_an_l_shaped_desk_falls_back_to_the_nearest_output() {
        // Below the external display but left of the laptop panel: a real
        // point on this desk that belongs to no output at all.
        let (index, x, y) = place(&desk(), 100.0, 2000.0, BAR_W, BAR_H).unwrap();
        assert_eq!(index, 1, "nearest output wins");
        assert!(LAPTOP.contains(x + BAR_W / 2.0, y + BAR_H / 2.0));
    }

    #[test]
    fn a_window_wider_than_its_output_does_not_panic() {
        let narrow = vec![Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 }];
        let (_, x, y) = place(&narrow, 50.0, 50.0, 400.0, 400.0).unwrap();
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn place_needs_at_least_one_output() {
        assert!(place(&[], 0.0, 0.0, BAR_W, BAR_H).is_none());
    }

    #[test]
    fn anchor_names_round_trip() {
        for anchor in [
            PanelAnchor::TopLeft,
            PanelAnchor::TopCenter,
            PanelAnchor::TopRight,
            PanelAnchor::BottomLeft,
            PanelAnchor::BottomCenter,
            PanelAnchor::BottomRight,
        ] {
            assert_eq!(PanelAnchor::parse(anchor.name()), Some(anchor));
        }
    }
}
