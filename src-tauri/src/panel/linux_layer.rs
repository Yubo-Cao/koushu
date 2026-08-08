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

use gtk_layer_shell::{Edge, Layer, LayerShell};

use super::{PanelAnchor, PanelStatus};

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

    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        gtk_window.set_anchor(edge, false);
        gtk_window.set_layer_shell_margin(edge, 0);
    }

    let vertical = if anchor.is_top() { Edge::Top } else { Edge::Bottom };
    gtk_window.set_anchor(vertical, true);
    gtk_window.set_layer_shell_margin(vertical, margin);

    match anchor.horizontal() {
        Some(true) => {
            gtk_window.set_anchor(Edge::Left, true);
            gtk_window.set_layer_shell_margin(Edge::Left, margin);
        }
        Some(false) => {
            gtk_window.set_anchor(Edge::Right, true);
            gtk_window.set_layer_shell_margin(Edge::Right, margin);
        }
        // Anchoring neither side leaves the surface centred on that axis.
        None => {}
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
