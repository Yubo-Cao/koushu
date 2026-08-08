//! Push-to-talk: a global hotkey that reports both press and release.
//!
//! Release is the hard requirement, and it is what rules out the usual
//! cross-platform hotkey crates. Tauri's global-shortcut plugin uses
//! `XGrabKey` on Linux, which a Wayland compositor never delivers to a
//! Wayland-native window, and Carbon `RegisterEventHotKey` on macOS, which
//! only ever fires on press.
//!
//! So each platform uses its own native mechanism, behind one interface:
//!
//! | Platform | Mechanism | Requirement |
//! |---|---|---|
//! | Linux (preferred) | XDG Portal `GlobalShortcuts` | non-empty app id, i.e. an installed `.desktop` |
//! | Linux (fallback) | `evdev` on `/dev/input/event*` | membership of the `input` group |
//! | macOS | `NSEvent` global monitor | Accessibility permission |

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(target_os = "linux")]
mod linux_evdev;
#[cfg(target_os = "linux")]
mod linux_portal;
#[cfg(target_os = "macos")]
mod macos;

/// Which mechanism ended up serving the hotkey. Surfaced in the UI so the
/// user can tell why a key is or is not being seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyBackend {
    /// XDG Desktop Portal. User-visible and remappable in system settings.
    Portal,
    /// Direct evdev read. Works anywhere, but sees all keyboard traffic.
    Evdev,
    /// macOS NSEvent global monitor.
    NsEvent,
    /// Nothing available; push-to-talk is off.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PttEdge {
    Pressed,
    Released,
}

/// Report of what the platform managed to set up, including why a preferred
/// backend was skipped. The `detail` is shown to the user rather than logged
/// and forgotten: "push-to-talk silently does nothing" is the worst outcome.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub backend: HotkeyBackend,
    pub trigger: String,
    pub detail: String,
}

/// A running push-to-talk listener. Dropping it releases the hotkey.
pub struct HotkeyListener {
    stop: Arc<AtomicBool>,
    pub status: HotkeyStatus,
    #[allow(dead_code)]
    join: Option<std::thread::JoinHandle<()>>,
}

impl Drop for HotkeyListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The platform threads poll `stop` and are detached rather than
        // joined: the portal thread parks on a D-Bus signal stream and evdev
        // parks on a blocking read, so neither is guaranteed to wake promptly.
        // They exit on their own once `stop` is observed.
    }
}

/// Ask the platform for whatever permission the hotkey needs, prompting if it
/// has not been answered yet. Returns whether it is now granted.
///
/// macOS needs Accessibility for CGEventTap. Linux needs nothing up front: the
/// portal prompts on its own when a shortcut is bound, and the evdev fallback
/// depends on group membership, which no runtime prompt can grant.
pub fn request_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::request_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Whether the hotkey permission is currently held.
pub fn has_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Start listening. `on_edge` is called from a background thread.
///
/// Linux tries the portal first and falls back to evdev, so a `tauri dev`
/// build (no installed `.desktop`, hence no app id) still works.
pub fn start<F>(trigger: &str, on_edge: F) -> HotkeyListener
where
    F: Fn(PttEdge) + Send + Sync + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let on_edge = Arc::new(on_edge);

    #[cfg(target_os = "linux")]
    {
        match linux_portal::start(trigger, Arc::clone(&stop), Arc::clone(&on_edge)) {
            Ok((status, join)) => {
                return HotkeyListener {
                    stop,
                    status,
                    join: Some(join),
                }
            }
            Err(portal_err) => match linux_evdev::start(trigger, Arc::clone(&stop), on_edge) {
                Ok((status, join)) => {
                    return HotkeyListener {
                        stop,
                        status: HotkeyStatus {
                            detail: format!(
                                "{} (portal unavailable: {portal_err})",
                                status.detail
                            ),
                            ..status
                        },
                        join: Some(join),
                    }
                }
                Err(evdev_err) => {
                    return HotkeyListener {
                        stop,
                        status: HotkeyStatus {
                            backend: HotkeyBackend::Unavailable,
                            trigger: trigger.to_string(),
                            detail: format!(
                                "No push-to-talk backend. Portal: {portal_err}. evdev: {evdev_err}."
                            ),
                            ..default_status(trigger)
                        },
                        join: None,
                    }
                }
            },
        }
    }

    #[cfg(target_os = "macos")]
    {
        match macos::start(trigger, Arc::clone(&stop), on_edge) {
            Ok((status, join)) => {
                return HotkeyListener {
                    stop,
                    status,
                    join: Some(join),
                }
            }
            Err(err) => {
                return HotkeyListener {
                    stop,
                    status: HotkeyStatus {
                        backend: HotkeyBackend::Unavailable,
                        trigger: trigger.to_string(),
                        detail: err,
                    },
                    join: None,
                }
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = on_edge;
        HotkeyListener {
            stop,
            status: HotkeyStatus {
                backend: HotkeyBackend::Unavailable,
                trigger: trigger.to_string(),
                detail: "Push-to-talk is only implemented for Linux and macOS.".to_string(),
            },
            join: None,
        }
    }
}

#[allow(dead_code)]
fn default_status(trigger: &str) -> HotkeyStatus {
    HotkeyStatus {
        backend: HotkeyBackend::Unavailable,
        trigger: trigger.to_string(),
        detail: String::new(),
    }
}
