//! Turn the voice bar into a real `wlr-layer-shell` panel.
//!
//! KWin implements `zwlr_layer_shell_v1` (v5 on Plasma 6.7), so this works on
//! KDE Wayland as well as wlroots compositors. It is not available on X11 —
//! there is no such protocol there — so the caller falls back to a positioned
//! always-on-top window.
//!
//! Timing matters: `init_layer_shell` has to run before the GTK window is
//! realized, because it swaps the underlying surface for a layer surface. The
//! voice bar is declared `visible: false` in tauri.conf.json precisely so it
//! is still unmapped when this runs at startup.

use gtk::prelude::*;
use gtk_layer_shell::{Edge, Layer, LayerShell};

use super::{PanelAnchor, PanelStatus};

/// One output: the GDK handle, plus its logical rectangle on the desktop.
pub struct Output {
    pub monitor: gtk::gdk::Monitor,
    pub rect: super::Rect,
}

/// Where the bar is, in the one coordinate space that is actually real here.
///
/// # Why not `tauri::Monitor`
///
/// On Linux, tao derives every "physical" number from GDK's *integer* scale
/// factor. That is not the compositor's scale: this desk runs a 2560x1600
/// laptop panel at 1.25, and GDK — which has no fractional scaling at all —
/// rounds that up and reports 2. So `monitor.position()` comes back as
/// (1024, 2880) for an output whose real origin is (512, 1440), a point in no
/// coordinate space the compositor has ever heard of.
///
/// Dividing that back out by the same scale recovers GDK's logical geometry,
/// which *is* correct — but only as long as the numerator and the denominator
/// came from the same monitor. They do not: window sizes carry the *window's*
/// scale (GDK gives a window the maximum scale of every output it overlaps)
/// while the code divided them by the *monitor's* scale. Measured on this
/// desk, a freshly shown window reports scale 2 while GDK still places it on
/// the scale-1 external display, so a 190pt pill measures as 380 and docks
/// half its own width away from the edge it was anchored to.
///
/// Reading GDK's logical geometry directly removes the round trip and the
/// mismatch with it. It is also the space everything else here already speaks:
/// wlr-layer-shell margins, KWin's global desktop layout, and kdotool's cursor
/// readings are all logical.
pub struct LogicalGeometry {
    /// Origin of the output the bar is on, in the global desktop.
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
    pub win_width: f64,
    pub win_height: f64,
}

/// Every enabled output, in GDK logical pixels.
pub fn outputs(window: &tauri::WebviewWindow) -> Result<Vec<Output>, String> {
    let gtk_window = window.gtk_window().map_err(|err| err.to_string())?;
    let display = gtk_window.display();
    let mut outputs = Vec::new();
    for index in 0..display.n_monitors() {
        let Some(monitor) = display.monitor(index) else {
            continue;
        };
        let geometry = monitor.geometry();
        outputs.push(Output {
            monitor,
            rect: super::Rect {
                x: geometry.x() as f64,
                y: geometry.y() as f64,
                width: geometry.width() as f64,
                height: geometry.height() as f64,
            },
        });
    }
    if outputs.is_empty() {
        return Err("no outputs".to_string());
    }
    Ok(outputs)
}

/// The output the bar is on, plus the bar's own size — all logical.
pub fn geometry(window: &tauri::WebviewWindow) -> Result<LogicalGeometry, String> {
    let gtk_window = window.gtk_window().map_err(|err| err.to_string())?;
    let display = gtk_window.display();

    // Order matters. The output layer-shell was explicitly told to use is the
    // truth when it exists; otherwise ask GDK where the surface ended up; and
    // only then fall back, because at startup the bar is deliberately still
    // unmapped and belongs to no output yet.
    let monitor = gtk_window
        .monitor()
        .or_else(|| {
            gtk_window
                .window()
                .and_then(|surface| display.monitor_at_window(&surface))
        })
        .or_else(|| display.primary_monitor())
        .or_else(|| display.monitor(0))
        .ok_or_else(|| "no monitor found for the voice bar".to_string())?;

    let geometry = monitor.geometry();
    let (win_width, win_height) = gtk_window.size();
    Ok(LogicalGeometry {
        origin_x: geometry.x() as f64,
        origin_y: geometry.y() as f64,
        width: geometry.width() as f64,
        height: geometry.height() as f64,
        win_width: win_width as f64,
        win_height: win_height as f64,
    })
}

/// Where the bar actually is, in global logical pixels.
///
/// GDK cannot answer this — Wayland never tells a client where the compositor
/// put its surface, so `gdk_window_get_origin` reports (0, 0) forever and
/// anything built on `outer_position()` is reading a constant. For a layer
/// surface anchored to the top-left corner, though, the margins we set *are*
/// the position, so the answer is recoverable exactly. `None` means the bar is
/// docked to some other corner, where a margin means an inset from that edge
/// rather than a coordinate.
pub fn current_position(window: &tauri::WebviewWindow) -> Option<(f64, f64)> {
    let gtk_window = window.gtk_window().ok()?;
    if !gtk_window.is_layer_window() {
        return None;
    }
    if !gtk_window.is_anchor(Edge::Top) || !gtk_window.is_anchor(Edge::Left) {
        return None;
    }
    let geometry = geometry(window).ok()?;
    Some((
        geometry.origin_x + gtk_window.layer_shell_margin(Edge::Left) as f64,
        geometry.origin_y + gtk_window.layer_shell_margin(Edge::Top) as f64,
    ))
}

/// Move a layer surface to an absolute position on the desktop, in logical
/// pixels, spanning every output.
///
/// Wayland forbids a client from setting its own window position — that is why
/// `set_position` silently does nothing and the bar sat wherever KWin dropped
/// it. layer-shell is the exception: anchoring to the top-left corner turns the
/// top and left margins into absolute coordinates, which is what makes
/// dragging possible at all here.
///
/// Those margins are measured from the output's own edge, though, not from the
/// desktop origin. On a single screen the two are the same and the difference
/// never shows; with a second output at (512, 1440) it is the whole ball game.
/// So the caller works in desktop coordinates, and this converts.
pub fn move_to(window: &tauri::WebviewWindow, x: i32, y: i32) -> Result<(), String> {
    let gtk_window = window.gtk_window().map_err(|err| err.to_string())?;
    if !gtk_window.is_layer_window() {
        return Err("voice bar is not a layer-shell surface".to_string());
    }

    let outputs = outputs(window)?;
    let (win_width, win_height) = gtk_window.size();
    let rects: Vec<super::Rect> = outputs.iter().map(|output| output.rect).collect();
    let (index, x, y) = super::place(
        &rects,
        x as f64,
        y as f64,
        win_width as f64,
        win_height as f64,
    )
    .ok_or_else(|| "no output to place the voice bar on".to_string())?;
    let target = &outputs[index];

    // Re-target only on an actual change: gtk_layer_set_monitor remaps the
    // surface, and doing that on every frame of a drag would strobe the bar.
    if gtk_window.monitor().as_ref() != Some(&target.monitor) {
        gtk_window.set_monitor(&target.monitor);
    }

    gtk_window.set_anchor(Edge::Top, true);
    gtk_window.set_anchor(Edge::Left, true);
    gtk_window.set_anchor(Edge::Bottom, false);
    gtk_window.set_anchor(Edge::Right, false);
    gtk_window.set_layer_shell_margin(Edge::Left, (x - target.rect.x).round() as i32);
    gtk_window.set_layer_shell_margin(Edge::Top, (y - target.rect.y).round() as i32);
    Ok(())
}

pub fn anchor(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
) -> Result<PanelStatus, String> {
    // gdk reports the backend actually in use; on X11 there is no layer-shell.
    if !gtk_layer_shell::is_supported() {
        return Err(
            "compositor does not implement zwlr_layer_shell_v1 (X11 session?)".to_string(),
        );
    }

    let gtk_window = window.gtk_window().map_err(|err| err.to_string())?;

    if !gtk_window.is_layer_window() {
        gtk_window.init_layer_shell();
    }

    // Overlay sits above normal and fullscreen windows, which is what a
    // push-to-talk indicator needs: it has to stay visible over whatever the
    // user is dictating into.
    gtk_window.set_layer(Layer::Overlay);
    gtk_window.set_namespace("fun-asr-voice-bar");

    // Never take keyboard focus. The bar appears while another application is
    // focused and must paste back into it; grabbing the keyboard would both
    // break that and swallow the user's own typing.
    gtk_window.set_keyboard_mode(gtk_layer_shell::KeyboardMode::None);

    // Compute the whole target state first, then apply it edge by edge.
    //
    // Clearing every anchor before setting the new ones passes through a state
    // with no anchors at all. Re-anchoring a live surface that way made the
    // window vanish, so each edge is written once to its final value and the
    // surface is never left un-anchored.
    let vertical = if anchor.is_top() { Edge::Top } else { Edge::Bottom };
    let horizontal = anchor.horizontal().map(|left| {
        if left {
            Edge::Left
        } else {
            Edge::Right
        }
    });

    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        let wanted = edge == vertical || Some(edge) == horizontal;
        gtk_window.set_anchor(edge, wanted);
        gtk_window.set_layer_shell_margin(edge, if wanted { margin } else { 0 });
    }

    // Deliberately no exclusive zone: the bar is transient and should overlay
    // the screen rather than permanently shrink everyone else's workspace.
    gtk_window.set_exclusive_zone(0);

    Ok(PanelStatus {
        anchored: true,
        layer_shell: true,
        detail: "Anchored as a wlr-layer-shell overlay panel. Stays above other windows and \
                 never takes keyboard focus."
            .to_string(),
    })
}
