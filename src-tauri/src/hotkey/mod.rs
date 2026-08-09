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
///
/// `ok` exists because "the call returned without an error" and "the chord the
/// user asked for is now live" are different questions, and only the second one
/// matters. The portal in particular answers the first cheerfully while
/// quietly keeping a binding the user is trying to replace.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub backend: HotkeyBackend,
    /// The chord that was asked for, canonicalised.
    pub trigger: String,
    /// Whether a listener is running on exactly `trigger`. False means the
    /// hotkey will not fire, or will fire on something else.
    pub ok: bool,
    /// What the desktop says is bound, in the desktop's own words and
    /// language. Portal only; the other backends bind what they are told.
    pub bound_description: Option<String>,
    pub detail: String,
}

/// Modifiers in the order a chord is written, so two spellings of the same
/// chord normalise to the same string and can be compared.
const MODIFIER_ORDER: [&str; 4] = ["CTRL", "ALT", "SHIFT", "LOGO"];

/// The canonical spelling of a modifier, or `None` if the token is not one.
fn modifier_name(upper: &str) -> Option<&'static str> {
    Some(match upper {
        "CTRL" | "CONTROL" => "CTRL",
        // Option is the Mac name for the same physical key.
        "ALT" | "OPTION" => "ALT",
        "SHIFT" => "SHIFT",
        // LOGO is what the XDG shortcuts spec calls it; the rest are the names
        // the same key goes by on other desktops and keyboards.
        "SUPER" | "META" | "LOGO" | "CMD" | "COMMAND" | "WIN" | "WINDOWS" => "LOGO",
        _ => return None,
    })
}

/// The XKB keysym name for a bindable main key, or `None` for one we refuse.
///
/// The set is deliberately small. Every key in it can be held down, survives a
/// keyboard layout change, and is not something a user needs for typing while
/// a modifier is also held. Notably absent: Escape, Tab, Return, Backspace and
/// the arrows, all of which are load-bearing in dialogs and text fields.
fn key_name(upper: &str) -> Option<String> {
    if upper == "SPACE" {
        return Some("space".to_string());
    }
    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_uppercase() {
            return Some((byte as char).to_ascii_lowercase().to_string());
        }
        if byte.is_ascii_digit() {
            return Some((byte as char).to_string());
        }
        return None;
    }
    let number: u8 = upper.strip_prefix('F')?.parse().ok()?;
    (1..=24).contains(&number).then(|| format!("F{number}"))
}

/// Canonicalise a chord, rejecting the ones that cannot serve as push-to-talk.
///
/// The output is the portal's format — `CTRL+ALT+space` — which is also what
/// the evdev and macOS parsers read, so one spelling drives all three.
///
/// Three shapes are refused, and the reasons are the point of the function:
///
///   - **No main key.** A chord of nothing but modifiers cannot be told apart
///     from the user reaching for a modifier on the way to something else.
///   - **No modifier.** A bare key is grabbed globally, so it would be eaten
///     out of every text field on the system.
///   - **An unsupported main key.** See `key_name`.
pub fn normalize_trigger(trigger: &str) -> Result<String, String> {
    let mut modifiers: Vec<&'static str> = Vec::new();
    let mut key: Option<String> = None;

    for part in trigger.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let upper = part.to_ascii_uppercase();
        if let Some(name) = modifier_name(&upper) {
            if !modifiers.contains(&name) {
                modifiers.push(name);
            }
            continue;
        }
        let named = key_name(&upper)
            .ok_or_else(|| format!("'{part}' cannot be used as a push-to-talk key."))?;
        if key.is_some() {
            return Err("A push-to-talk shortcut takes one key besides the modifiers.".to_string());
        }
        key = Some(named);
    }

    let key = key.ok_or_else(|| {
        "A push-to-talk shortcut needs a key besides the modifiers.".to_string()
    })?;
    if modifiers.is_empty() {
        return Err(
            "A push-to-talk shortcut needs at least one modifier, otherwise it would swallow the \
             key everywhere."
                .to_string(),
        );
    }
    modifiers.sort_by_key(|name| {
        MODIFIER_ORDER
            .iter()
            .position(|entry| entry == name)
            .unwrap_or(usize::MAX)
    });
    Ok(format!("{}+{key}", modifiers.join("+")))
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

    // Refuse a bad chord here rather than letting each backend discover it
    // separately, so the message the user sees is the same on every platform.
    let trigger = match normalize_trigger(trigger) {
        Ok(value) => value,
        Err(err) => {
            let _ = on_edge;
            return HotkeyListener {
                stop,
                status: unavailable(trigger, err),
                join: None,
            };
        }
    };
    let trigger = trigger.as_str();

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
                        status: unavailable(
                            trigger,
                            format!(
                                "No push-to-talk backend. Portal: {portal_err}. evdev: {evdev_err}."
                            ),
                        ),
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
                    status: unavailable(trigger, err),
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
            status: unavailable(
                trigger,
                "Push-to-talk is only implemented for Linux and macOS.".to_string(),
            ),
            join: None,
        }
    }
}

/// Nothing is listening. Every caller of this is a case where the hotkey will
/// not fire, so `ok` is false and the reason travels with it.
fn unavailable(trigger: &str, detail: String) -> HotkeyStatus {
    HotkeyStatus {
        backend: HotkeyBackend::Unavailable,
        trigger: trigger.to_string(),
        ok: false,
        bound_description: None,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_spelling_and_order() {
        assert_eq!(normalize_trigger("ctrl+alt+SPACE").unwrap(), "CTRL+ALT+space");
        assert_eq!(normalize_trigger("Alt + Ctrl + d").unwrap(), "CTRL+ALT+d");
        assert_eq!(normalize_trigger("cmd+shift+f5").unwrap(), "SHIFT+LOGO+F5");
        assert_eq!(normalize_trigger("super+meta+1").unwrap(), "LOGO+1");
    }

    #[test]
    fn keeps_the_shipped_default_stable() {
        // The stored setting and the default have to agree after a round trip,
        // or every launch would look like the user had changed the chord.
        assert_eq!(
            normalize_trigger("CTRL+ALT+space").unwrap(),
            "CTRL+ALT+space"
        );
    }

    #[test]
    fn refuses_chords_that_would_break_typing() {
        // Modifiers alone: nothing to press.
        assert!(normalize_trigger("CTRL+ALT").is_err());
        // Bare key: grabbed globally, so it disappears from every text field.
        assert!(normalize_trigger("space").is_err());
        assert!(normalize_trigger("a").is_err());
        // Keys that dialogs and text fields need.
        assert!(normalize_trigger("CTRL+ALT+Escape").is_err());
        assert!(normalize_trigger("CTRL+Tab").is_err());
        assert!(normalize_trigger("CTRL+ALT+Return").is_err());
        // Two main keys is not a chord anyone can hold.
        assert!(normalize_trigger("CTRL+a+b").is_err());
        assert!(normalize_trigger("").is_err());
        assert!(normalize_trigger("CTRL+ALT+F25").is_err());
    }
}
