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
    await_response(&conn, &request, Duration::from_secs(60))?;

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

    Ok((
        HotkeyStatus {
            backend: HotkeyBackend::Portal,
            trigger: trigger.to_string(),
            detail: "Bound through the desktop portal. Remappable in system settings.".to_string(),
        },
        join,
    ))
}
