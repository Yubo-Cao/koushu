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

/// The evdev code for a main key, by the name `normalize_trigger` produces
/// (already upper-cased by the caller).
///
/// The letters need a table. `input-event-codes.h` numbers them by where they
/// sit on a US keyboard, not by the alphabet — `KEY_A` is 30 and `KEY_B` is 48,
/// with the whole QWERTY top row in between — so deriving them by adding an
/// offset to `KEY_A` silently yields a different key for all 25 letters after
/// A. Digits are contiguous but start at `1` and wrap `0` around to the end,
/// and the function keys run F1..F10, then F11/F12, then F13 far away.
fn key_code(name: &str) -> Option<KeyCode> {
    if name == "SPACE" {
        return Some(KeyCode::KEY_SPACE);
    }
    if name.len() == 1 {
        let byte = name.as_bytes()[0];
        if byte.is_ascii_uppercase() {
            const LETTERS: [KeyCode; 26] = [
                KeyCode::KEY_A,
                KeyCode::KEY_B,
                KeyCode::KEY_C,
                KeyCode::KEY_D,
                KeyCode::KEY_E,
                KeyCode::KEY_F,
                KeyCode::KEY_G,
                KeyCode::KEY_H,
                KeyCode::KEY_I,
                KeyCode::KEY_J,
                KeyCode::KEY_K,
                KeyCode::KEY_L,
                KeyCode::KEY_M,
                KeyCode::KEY_N,
                KeyCode::KEY_O,
                KeyCode::KEY_P,
                KeyCode::KEY_Q,
                KeyCode::KEY_R,
                KeyCode::KEY_S,
                KeyCode::KEY_T,
                KeyCode::KEY_U,
                KeyCode::KEY_V,
                KeyCode::KEY_W,
                KeyCode::KEY_X,
                KeyCode::KEY_Y,
                KeyCode::KEY_Z,
            ];
            return Some(LETTERS[usize::from(byte - b'A')]);
        }
        return match byte {
            b'0' => Some(KeyCode::KEY_0),
            b'1'..=b'9' => Some(KeyCode::new(KeyCode::KEY_1.code() + u16::from(byte - b'1'))),
            _ => None,
        };
    }
    let number: u16 = name.strip_prefix('F')?.parse().ok()?;
    match number {
        1..=10 => Some(KeyCode::new(KeyCode::KEY_F1.code() + number - 1)),
        11 => Some(KeyCode::KEY_F11),
        12 => Some(KeyCode::KEY_F12),
        13..=24 => Some(KeyCode::new(KeyCode::KEY_F13.code() + number - 13)),
        _ => None,
    }
}

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
            other => {
                main = Some(
                    key_code(other).ok_or_else(|| format!("unsupported key '{other}' in trigger"))?,
                )
            }
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
            // Nothing mediates this binding, so what was asked for is what is
            // watched for. The cost is that a chord the desktop also uses will
            // fire both, which this backend has no way to find out about.
            ok: true,
            bound_description: None,
            detail: format!(
                "Reading {device_count} keyboard device(s) directly. Key events are matched and discarded in-process; nothing is stored or sent."
            ),
        },
        join,
    ))
}

#[cfg(test)]
mod tests {
    use super::{key_code, parse_trigger};
    use evdev::KeyCode;

    #[test]
    fn letters_are_not_alphabetical_in_evdev() {
        // The regression this table exists for: `KEY_A + (c - 'A')` gives 39
        // for J, which is KEY_APOSTROPHE, and 31 for B, which is KEY_S.
        assert_eq!(key_code("A"), Some(KeyCode::KEY_A));
        assert_eq!(key_code("B"), Some(KeyCode::KEY_B));
        assert_eq!(key_code("J"), Some(KeyCode::KEY_J));
        assert_eq!(key_code("Z"), Some(KeyCode::KEY_Z));
        assert_ne!(key_code("J"), Some(KeyCode::new(KeyCode::KEY_A.code() + 9)));
    }

    #[test]
    fn digits_wrap_zero_to_the_end() {
        assert_eq!(key_code("1"), Some(KeyCode::KEY_1));
        assert_eq!(key_code("9"), Some(KeyCode::KEY_9));
        assert_eq!(key_code("0"), Some(KeyCode::KEY_0));
    }

    #[test]
    fn function_keys_span_their_three_runs() {
        assert_eq!(key_code("F1"), Some(KeyCode::KEY_F1));
        assert_eq!(key_code("F10"), Some(KeyCode::KEY_F10));
        assert_eq!(key_code("F11"), Some(KeyCode::KEY_F11));
        assert_eq!(key_code("F12"), Some(KeyCode::KEY_F12));
        assert_eq!(key_code("F13"), Some(KeyCode::KEY_F13));
        assert_eq!(key_code("F24"), Some(KeyCode::KEY_F24));
        assert_eq!(key_code("F25"), None);
    }

    #[test]
    fn a_chord_parses_into_its_modifiers_and_key() {
        let (modifiers, main) = parse_trigger("CTRL+ALT+SHIFT+j").unwrap();
        assert_eq!(main, KeyCode::KEY_J);
        assert_eq!(modifiers.len(), 3);
        assert!(modifiers[0].contains(&KeyCode::KEY_LEFTCTRL));
        assert!(parse_trigger("CTRL+ALT+nope").is_err());
    }
}
