//! Compositor-side background blur, via `ext-background-effect-v1`.
//!
//! CSS `backdrop-filter` cannot do this. It samples the page's own compositing
//! result, so on a transparent, pill-sized window there is nothing behind the
//! content to sample and the filter is a no-op — which is exactly why the
//! voice bar looked like painted metal rather than glass. Only the compositor
//! can blur the desktop behind a surface.
//!
//! `ext_background_effect_manager_v1` is a cross-compositor staging protocol
//! (KWin 6.7 advertises it), so this is not KDE-specific the way
//! `org_kde_kwin_blur` would have been.

use std::ffi::c_void;

use wayland_client::backend::{Backend, ObjectId};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

// GTK owns the Wayland connection and the surface; we borrow both rather than
// opening a second connection, because a surface belongs to the connection
// that created it and cannot be used across another.
extern "C" {
    fn gdk_wayland_display_get_wl_display(display: *mut c_void) -> *mut c_void;
    fn gdk_wayland_window_get_wl_surface(window: *mut c_void) -> *mut c_void;
}

#[derive(Default)]
struct State {
    manager: Option<ExtBackgroundEffectManagerV1>,
}

impl Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wayland_client::protocol::wl_registry::WlRegistry,
        event: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            if interface == ExtBackgroundEffectManagerV1::interface().name {
                state.manager =
                    Some(registry.bind::<ExtBackgroundEffectManagerV1, _, _>(name, 1, qh, ()));
            }
        }
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        _: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectSurfaceV1,
        _: <ExtBackgroundEffectSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Ask the compositor to blur whatever is behind this window.
///
/// Best-effort by design: an unsupported compositor, an X11 session, or a
/// missing protocol all just mean no blur, never a failure the user sees.
pub fn enable_for(window: &tauri::WebviewWindow) -> Result<(), String> {
    let gtk_window = window.gtk_window().map_err(|err| err.to_string())?;

    use gtk::prelude::*;
    let gdk_window = gtk_window
        .window()
        .ok_or_else(|| "window is not realized yet".to_string())?;
    let display = gdk_window.display();

    // Both pointers belong to GTK; we only borrow them for the calls below.
    let (wl_display, wl_surface_ptr) = unsafe {
        let display_ptr = display.as_ptr() as *mut c_void;
        let window_ptr = gdk_window.as_ptr() as *mut c_void;
        (
            gdk_wayland_display_get_wl_display(display_ptr),
            gdk_wayland_window_get_wl_surface(window_ptr),
        )
    };
    if wl_display.is_null() || wl_surface_ptr.is_null() {
        return Err("not a Wayland surface".to_string());
    }

    // Wrap GTK's existing connection instead of opening our own.
    let backend = unsafe { Backend::from_foreign_display(wl_display as *mut _) };
    let conn = Connection::from_backend(backend);
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();

    let mut state = State::default();
    let _registry = conn.display().get_registry(&qh, ());
    queue
        .roundtrip(&mut state)
        .map_err(|err| format!("wayland roundtrip failed: {err}"))?;

    let manager = state
        .manager
        .ok_or_else(|| "compositor does not offer ext_background_effect_v1".to_string())?;

    let surface_id = unsafe {
        ObjectId::from_ptr(WlSurface::interface(), wl_surface_ptr as *mut _)
            .map_err(|err| format!("could not adopt the GTK surface: {err}"))?
    };
    let surface = WlSurface::from_id(&conn, surface_id)
        .map_err(|err| format!("could not wrap the GTK surface: {err}"))?;

    let effect = manager.get_background_effect(&surface, &qh, ());
    // A null region means "the whole surface", which is what a pill wants.
    effect.set_blur_region(None);
    surface.commit();
    conn.flush().map_err(|err| err.to_string())?;

    // The effect object must outlive this call or the compositor drops the
    // blur; the surface owns the lifetime for as long as the window exists.
    std::mem::forget(effect);
    Ok(())
}
