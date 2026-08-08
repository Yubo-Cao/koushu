//! Push-to-talk by reading `/dev/input/event*` directly.
//!
//! The fallback for when the portal is unavailable — most importantly during
//! development, where an uninstalled build has no app id for the portal to
//! accept. It bypasses the compositor entirely, so it works identically on
//! Wayland and X11 and gives exact press/release edges.
//!
//! The tradeoff is real and worth stating plainly: this reads every key event
//! from the keyboards it opens, not just the hotkey. Nothing is stored or sent
//! anywhere — events that are not part of the configured chord are discarded
//! immediately — but it does require membership of the `input` group, and it
//! is the reason the portal is preferred whenever it will have us.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

use evdev::{Device, EventSummary, KeyCode};

use super::{HotkeyBackend, HotkeyStatus, PttEdge};

/// Translate a portal-style trigger string ("CTRL+ALT+space") into key codes.
/// Returns the modifiers that must be held and the main key.
fn parse_trigger(trigger: &str) -> Result<(Vec<Vec<KeyCode>>, KeyCode), String> {
    let mut modifiers: Vec<Vec<KeyCode>> = Vec::new();
    let mut main: Option<KeyCode> = None;

    for part in trigger.split('+') {
        let part = part.trim();
        match part.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => {
                modifiers.push(vec![KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL])
            }
            "ALT" => modifiers.push(vec![KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT]),
            "SHIFT" => modifiers.push(vec![KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_RIGHTSHIFT]),
            "SUPER" | "META" | "LOGO" => {
                modifiers.push(vec![KeyCode::KEY_LEFTMETA, KeyCode::KEY_RIGHTMETA])
            }
            "SPACE" => main = Some(KeyCode::KEY_SPACE),
            other if other.len() == 1 && other.is_ascii() => {
                let c = other.as_bytes()[0];
                let code = match c {
                    b'A'..=b'Z' => KeyCode::new(KeyCode::KEY_A.code() + u16::from(c - b'A')),
                    _ => return Err(format!("unsupported key '{other}' in trigger")),
                };
                main = Some(code);
            }
            other => return Err(format!("unsupported key '{other}' in trigger")),
        }
    }

    let main = main.ok_or_else(|| format!("no main key in trigger '{trigger}'"))?;
    Ok((modifiers, main))
}

/// Keyboards, identified by advertising the main key we care about.
///
/// Paths are returned alongside so the poll loop can spot devices that appear
/// later; without rescanning, plugging in a keyboard after startup would
/// silently stop push-to-talk from working on it.
fn keyboards(main: KeyCode) -> Vec<(std::path::PathBuf, Device)> {
    evdev::enumerate()
        .filter(|(_, device)| {
            device
                .supported_keys()
                .is_some_and(|keys| keys.contains(main))
        })
        .collect()
}

/// How often to rescan `/dev/input` for keyboards that appeared after startup.
const RESCAN_INTERVAL: Duration = Duration::from_secs(3);

pub fn start<F>(
    trigger: &str,
    stop: Arc<AtomicBool>,
    on_edge: Arc<F>,
) -> Result<(HotkeyStatus, JoinHandle<()>), String>
where
    F: Fn(PttEdge) + Send + Sync + 'static,
{
    let (modifiers, main) = parse_trigger(trigger)?;
    let devices = keyboards(main);
    if devices.is_empty() {
        return Err(
            "no readable keyboard in /dev/input (is this user in the `input` group?)".to_string(),
        );
    }
    let device_count = devices.len();

    let join = std::thread::spawn(move || {
        // Modifier state is tracked across all devices together, since a
        // chord can legitimately span two of them (an external keyboard's
        // Ctrl with the builtin's Space is unusual, but a laptop reporting
        // one physical keyboard as several event nodes is not).
        let mut devices = devices;
        for (_, device) in &devices {
            let _ = device.set_nonblocking(true);
        }
        let mut held: std::collections::HashSet<u16> = std::collections::HashSet::new();
        let mut engaged = false;
        let mut last_scan = std::time::Instant::now();

        while !stop.load(Ordering::SeqCst) {
            if last_scan.elapsed() >= RESCAN_INTERVAL {
                last_scan = std::time::Instant::now();
                let known: std::collections::HashSet<_> =
                    devices.iter().map(|(path, _)| path.clone()).collect();
                for (path, device) in keyboards(main) {
                    if !known.contains(&path) {
                        let _ = device.set_nonblocking(true);
                        devices.push((path, device));
                    }
                }
                // Drop devices that have gone away, so an unplugged keyboard
                // does not keep erroring forever.
                devices.retain(|(path, _)| path.exists());
            }

            let mut saw_event = false;
            for (_, device) in &mut devices {
                let Ok(events) = device.fetch_events() else {
                    continue;
                };
                for event in events {
                    let EventSummary::Key(_, key, value) = event.destructure() else {
                        continue;
                    };
                    saw_event = true;
                    match value {
                        0 => {
                            held.remove(&key.code());
                        }
                        1 => {
                            held.insert(key.code());
                        }
                        // 2 is auto-repeat; the key is still down.
                        _ => continue,
                    }

                    let mods_ok = modifiers
                        .iter()
                        .all(|group| group.iter().any(|k| held.contains(&k.code())));
                    let main_down = held.contains(&main.code());
                    let now = mods_ok && main_down;

                    if now && !engaged {
                        engaged = true;
                        on_edge(PttEdge::Pressed);
                    } else if !now && engaged {
                        engaged = false;
                        on_edge(PttEdge::Released);
                    }
                }
            }
            if !saw_event {
                std::thread::sleep(Duration::from_millis(8));
            }
        }
    });

    Ok((
        HotkeyStatus {
            backend: HotkeyBackend::Evdev,
            trigger: trigger.to_string(),
            detail: format!(
                "Reading {device_count} keyboard device(s) directly. Key events are matched and discarded in-process; nothing is stored or sent."
            ),
        },
        join,
    ))
}
