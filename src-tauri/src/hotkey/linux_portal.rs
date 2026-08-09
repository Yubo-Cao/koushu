//! Push-to-talk via the XDG Desktop Portal `GlobalShortcuts` interface.
//!
//! This is the only sanctioned way to grab a global key under Wayland: the
//! compositor owns input, and the portal mediates. It is also the nicest for
//! users, since the binding shows up in the desktop's own shortcut settings.
//!
//! Portal v2 emits both `Activated` and `Deactivated`, which is what makes
//! push-to-talk possible at all.
//!
//! The catch: the portal refuses a session when it cannot resolve an app id
//! (`org.freedesktop.portal.Error.NotAllowed: An app id is required`). A
//! non-sandboxed build only gets one once it is installed with a matching
//! `.desktop` file, so an uninstalled `cargo run` build always lands on the
//! evdev fallback. That is expected, not a bug.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::proxy;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::{HotkeyBackend, HotkeyStatus, PttEdge};

#[proxy(
    interface = "org.freedesktop.portal.GlobalShortcuts",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait GlobalShortcuts {
    fn create_session(
        &self,
        options: std::collections::HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;

    fn bind_shortcuts(
        &self,
        session_handle: &ObjectPath<'_>,
        shortcuts: Vec<(&str, std::collections::HashMap<&str, Value<'_>>)>,
        parent_window: &str,
        options: std::collections::HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;

    #[zbus(signal)]
    fn activated(
        &self,
        session_handle: OwnedObjectPath,
        shortcut_id: String,
        timestamp: u64,
        options: std::collections::HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn deactivated(
        &self,
        session_handle: OwnedObjectPath,
        shortcut_id: String,
        timestamp: u64,
        options: std::collections::HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.portal.Request",
    default_service = "org.freedesktop.portal.Desktop"
)]
trait Request {
    #[zbus(signal)]
    fn response(
        &self,
        response: u32,
        results: std::collections::HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}

const SHORTCUT_ID: &str = "push_to_talk";

/// What the portal says is bound to our shortcut, in the desktop's own words.
///
/// `BindShortcuts` answers with the shortcuts it ended up with, and that answer
/// is the only honest source for what is live. `preferred_trigger` is, as the
/// name says, a preference: the XDG spec has the portal keep a binding that
/// already exists, and KDE does exactly that — it returns success while leaving
/// the old chord in place. Without reading this back, changing the hotkey would
/// look like it worked every single time.
fn bound_description(results: &std::collections::HashMap<String, OwnedValue>) -> Option<String> {
    let shortcuts: Vec<(String, std::collections::HashMap<String, OwnedValue>)> =
        results.get("shortcuts")?.try_clone().ok()?.try_into().ok()?;
    let (_, meta) = shortcuts.into_iter().find(|(id, _)| id == SHORTCUT_ID)?;
    let description = meta.get("trigger_description")?;
    String::try_from(description.try_clone().ok()?).ok()
}

/// Modifier tokens as desktops spell them, mapped onto our own names.
///
/// Only the modifiers are worth a table: every desktop writes them in Latin
/// letters or the Mac symbols even when the rest of the string is localised
/// (KDE in Chinese returns `Ctrl+Alt+空格`).
fn description_modifier(token: &str) -> Option<&'static str> {
    Some(match token.to_ascii_uppercase().as_str() {
        "CTRL" | "CONTROL" | "STRG" | "^" | "⌃" => "CTRL",
        "ALT" | "OPTION" | "⌥" => "ALT",
        "SHIFT" | "⇧" => "SHIFT",
        "META" | "SUPER" | "LOGO" | "WIN" | "WINDOWS" | "CMD" | "COMMAND" | "⌘" => "LOGO",
        _ => return None,
    })
}

/// Whether the desktop's description names the chord we asked for.
///
/// `None` means "cannot tell", which is a real answer here and is not the same
/// as "no". The description is written for humans and is translated, so the one
/// key whose name varies by language — the space bar — cannot always be
/// checked. Letters, digits and function keys are printed as themselves in
/// every locale, so a difference there is a genuine mismatch and is reported as
/// one; guessing either way would defeat the point of asking.
fn describes_chord(requested: &str, description: &str) -> Option<bool> {
    let mut wanted_modifiers: Vec<&str> = Vec::new();
    let mut wanted_key = "";
    for part in requested.split('+') {
        // `requested` has been through `normalize_trigger`, so anything that is
        // not one of our modifier names is the main key.
        if matches!(part, "CTRL" | "ALT" | "SHIFT" | "LOGO") {
            wanted_modifiers.push(part);
        } else {
            wanted_key = part;
        }
    }

    let mut modifiers: Vec<&str> = Vec::new();
    let mut keys: Vec<&str> = Vec::new();
    for token in description.split('+') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match description_modifier(token) {
            Some(name) => modifiers.push(name),
            None => keys.push(token),
        }
    }
    if keys.len() != 1 {
        return None;
    }
    modifiers.sort_unstable();
    modifiers.dedup();
    wanted_modifiers.sort_unstable();
    if modifiers != wanted_modifiers {
        return Some(false);
    }

    let key = keys[0];
    if key.eq_ignore_ascii_case(wanted_key) {
        return Some(true);
    }
    // A localised name for a key we cannot recognise. Only `space` has one, so
    // if that is not what we asked for, this is a different key.
    if !key.is_ascii() {
        return (wanted_key != "space").then_some(false);
    }
    Some(false)
}

/// Wait for a portal `Request` to answer. Every portal call is asynchronous:
/// the method returns a request handle and the real answer arrives later as a
/// `Response` signal.
fn await_response(
    conn: &Connection,
    handle: &OwnedObjectPath,
    timeout: Duration,
) -> Result<std::collections::HashMap<String, OwnedValue>, String> {
    let request = RequestProxyBlocking::builder(conn)
        .path(handle.clone())
        .map_err(|err| err.to_string())?
        .build()
        .map_err(|err| err.to_string())?;
    let mut signals = request.receive_response().map_err(|err| err.to_string())?;

    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Some(signal) = signals.next() {
            let args = signal.args().map_err(|err| err.to_string())?;
            return match args.response {
                0 => Ok(args.results),
                1 => Err("cancelled by the user".to_string()),
                other => Err(format!("portal returned response code {other}")),
            };
        }
    }
    Err("timed out waiting for the portal".to_string())
}

pub fn start<F>(
    trigger: &str,
    stop: Arc<AtomicBool>,
    on_edge: Arc<F>,
) -> Result<(HotkeyStatus, JoinHandle<()>), String>
where
    F: Fn(PttEdge) + Send + Sync + 'static,
{
    let conn = Connection::session().map_err(|err| err.to_string())?;
    let shortcuts = GlobalShortcutsProxyBlocking::new(&conn).map_err(|err| err.to_string())?;

    let mut options = std::collections::HashMap::new();
    options.insert("handle_token", Value::from("fun_asr_ptt"));
    options.insert("session_handle_token", Value::from("fun_asr_ptt_session"));
    let request = shortcuts
        .create_session(options)
        .map_err(|err| err.to_string())?;
    let results = await_response(&conn, &request, Duration::from_secs(10))?;

    let session: OwnedObjectPath = results
        .get("session_handle")
        .ok_or_else(|| "portal did not return a session handle".to_string())
        .and_then(|value| {
            // Portals return this either as an object path or as a string.
            String::try_from(value.try_clone().map_err(|e| e.to_string())?)
                .map_err(|err| err.to_string())
                .and_then(|s| OwnedObjectPath::try_from(s).map_err(|err| err.to_string()))
        })?;

    let mut meta = std::collections::HashMap::new();
    meta.insert("description", Value::from("Fun ASR: hold to talk"));
    meta.insert("preferred_trigger", Value::from(trigger));
    let mut bind_options = std::collections::HashMap::new();
    bind_options.insert("handle_token", Value::from("fun_asr_ptt_bind"));
    let request = shortcuts
        .bind_shortcuts(&session.as_ref(), vec![(SHORTCUT_ID, meta)], "", bind_options)
        .map_err(|err| err.to_string())?;
    // Binding can prompt the user, so allow a generous window.
    let bound = await_response(&conn, &request, Duration::from_secs(60))?;
    let described = bound_description(&bound);

    let mut activated = shortcuts.receive_activated().map_err(|e| e.to_string())?;
    let mut deactivated = shortcuts.receive_deactivated().map_err(|e| e.to_string())?;

    // One thread per signal stream. `next()` blocks, so draining both from a
    // single loop would let a pending press block delivery of the release —
    // and one missed edge would swap the two forever after.
    let release_stop = Arc::clone(&stop);
    let release_edge = Arc::clone(&on_edge);
    std::thread::spawn(move || {
        while !release_stop.load(Ordering::SeqCst) {
            let Some(signal) = deactivated.next() else {
                break;
            };
            if signal
                .args()
                .map(|a| a.shortcut_id == SHORTCUT_ID)
                .unwrap_or(false)
            {
                release_edge(PttEdge::Released);
            }
        }
    });

    let join = std::thread::spawn(move || {
        // Keep the connection alive for as long as we listen; dropping it
        // would end the portal session and silently unbind the shortcut.
        let _conn = conn;
        while !stop.load(Ordering::SeqCst) {
            let Some(signal) = activated.next() else {
                break;
            };
            if signal
                .args()
                .map(|a| a.shortcut_id == SHORTCUT_ID)
                .unwrap_or(false)
            {
                on_edge(PttEdge::Pressed);
            }
        }
    });

    let matches = described
        .as_deref()
        .map(|description| describes_chord(trigger, description));
    let ok = !matches!(matches, Some(Some(false)));
    let detail = match (&described, matches) {
        (Some(description), Some(Some(false))) => format!(
            "The desktop kept its own binding for this shortcut: {description}. A shortcut the \
             portal has already bound can only be changed in the desktop's own shortcut settings."
        ),
        (Some(description), _) => format!(
            "Bound through the desktop portal as {description}. Remappable in system settings."
        ),
        (None, _) => {
            "Bound through the desktop portal. Remappable in system settings.".to_string()
        }
    };

    Ok((
        HotkeyStatus {
            backend: HotkeyBackend::Portal,
            trigger: trigger.to_string(),
            ok,
            bound_description: described,
            detail,
        },
        join,
    ))
}

#[cfg(test)]
mod tests {
    use super::describes_chord;

    #[test]
    fn reads_a_matching_description() {
        assert_eq!(describes_chord("CTRL+ALT+space", "Ctrl+Alt+Space"), Some(true));
        assert_eq!(describes_chord("CTRL+ALT+d", "Ctrl+Alt+D"), Some(true));
        assert_eq!(describes_chord("SHIFT+LOGO+F5", "Meta+Shift+F5"), Some(true));
    }

    #[test]
    fn catches_the_desktop_keeping_its_own_binding() {
        // The case this whole readback exists for: KDE answers a request for
        // Ctrl+Alt+D with success while still holding Ctrl+Alt+Space, and says
        // so in the session language.
        assert_eq!(describes_chord("CTRL+ALT+d", "Ctrl+Alt+空格"), Some(false));
        assert_eq!(describes_chord("CTRL+ALT+d", "Ctrl+Alt+Space"), Some(false));
        assert_eq!(describes_chord("CTRL+ALT+space", "Ctrl+Shift+Space"), Some(false));
    }

    #[test]
    fn admits_when_it_cannot_tell() {
        // Asked for space, given a translated name for some key: it is probably
        // the space bar, and claiming a mismatch would be a false alarm.
        assert_eq!(describes_chord("CTRL+ALT+space", "Ctrl+Alt+空格"), None);
        assert_eq!(describes_chord("CTRL+ALT+space", "Ctrl+Alt"), None);
    }
}
