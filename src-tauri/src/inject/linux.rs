//! Linux delivery: uinput keystrokes for ASCII, clipboard plus a chord for
//! everything else.
//!
//! # Why not an input method
//!
//! `zwp_input_method_v2` would be the correct primitive: it commits arbitrary
//! UTF-8 into the focused text field, reports activate/deactivate so focus is
//! *known* rather than guessed, and supports preedit — which is exactly
//! "show the words as they are being spoken, then commit". Two facts rule it
//! out here. KWin on this machine advertises only `zwp_input_method_v1` (and
//! `zwp_text_input_manager_v2/v3`, which is the client half and cannot observe
//! other applications). And a seat has one input method: taking it would
//! displace the user's fcitx5, breaking Pinyin input for the sake of dictation.
//!
//! So the transport is uinput, via ydotool, and its keymap limits decide
//! everything below.

use std::env;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::apps::{chord_for, Chord};
use super::{InjectReport, Method, Target};

/// evdev keycodes. `linux/input-event-codes.h`.
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_V: u16 = 47;

/// How long to wait after handing text to `wl-copy` before pasting.
///
/// On Wayland the clipboard is owned by a live process rather than the server,
/// so the paste cannot succeed until `wl-copy` has forked and taken ownership
/// of the selection. Pasting immediately races that handoff and yields the
/// *previous* clipboard contents, which looks exactly like a stale transcript.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(120);

/// ydotool talks to `ydotoold` over a unix socket, and the daemon's default
/// location is not the same across distributions. On this machine it is
/// `$XDG_RUNTIME_DIR/.ydotool_socket` while ydotool's compiled-in default is
/// `/tmp/.ydotool_socket`; when they disagree ydotool exits non-zero with a
/// connection error, which reads as "injection is broken" rather than
/// "misconfigured socket". Passing it explicitly removes the ambiguity.
fn ydotool() -> Command {
    let mut cmd = Command::new("ydotool");
    if env::var_os("YDOTOOL_SOCKET").is_none() {
        if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
            let candidate = std::path::Path::new(&runtime).join(".ydotool_socket");
            if candidate.exists() {
                cmd.env("YDOTOOL_SOCKET", candidate);
            }
        }
    }
    cmd
}

fn command_exists(program: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {program}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn is_wayland() -> bool {
    matches!(env::var("XDG_SESSION_TYPE").as_deref(), Ok("wayland"))
        || env::var("WAYLAND_DISPLAY").is_ok()
}

/// Ask the compositor which window has focus.
///
/// KWin exposes this over D-Bus but only for a window it can name, so the uuid
/// comes from `kdotool` first. `getWindowInfo` returns a flat `key: value`
/// listing; `resourceClass` is the value [`chord_for`] matches on.
pub fn capture_target() -> Target {
    let mut target = Target::default();

    let Some(uuid) = run_capture("kdotool", &["getactivewindow"]) else {
        // No kdotool: fall back to nothing rather than guessing. An unknown
        // target still delivers, it just uses the default chord.
        return target;
    };
    let uuid = uuid.trim().to_string();
    if uuid.is_empty() {
        return target;
    }

    let qdbus = if command_exists("qdbus6") { "qdbus6" } else { "qdbus" };
    if let Some(info) = run_capture(
        qdbus,
        &["org.kde.KWin", "/KWin", "org.kde.KWin.getWindowInfo", &uuid],
    ) {
        for line in info.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim() {
                "resourceClass" => target.app_id = Some(value.to_string()),
                "caption" => target.app_name = Some(value.to_string()),
                "pid" => target.pid = value.parse().ok(),
                _ => {}
            }
        }
    }

    if target.app_id.is_none() {
        target.app_id = run_capture("kdotool", &["getwindowclassname", &uuid])
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    // accepts_text stays None on purpose. See the field's documentation: the
    // only way to learn it on Wayland is to become the input method.
    target
}

fn run_capture(program: &str, args: &[&str]) -> Option<String> {
    if !command_exists(program) {
        return None;
    }
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn inject(text: &str, target: &Target, keep_clipboard: bool) -> InjectReport {
    // Typing avoids touching the clipboard, but only ASCII survives the keymap,
    // so it is offered only when the caller asked to spare the clipboard and
    // the text can actually make it through intact.
    if keep_clipboard && super::is_typeable(text) {
        match type_text(text) {
            Ok(()) => {
                return InjectReport {
                    delivered: true,
                    method: Some(Method::Typed),
                    chord: None,
                    clipboard_used: false,
                    target: target.clone(),
                    message: "Typed into the focused window.".to_string(),
                }
            }
            Err(err) => {
                // Fall through to the clipboard path rather than giving up: a
                // failed uinput write should cost latency, not the transcript.
                let _ = err;
            }
        }
    }

    let chord = chord_for(target.app_id.as_deref());
    match paste_text(text, chord) {
        Ok(()) => InjectReport {
            delivered: true,
            method: Some(Method::Pasted),
            chord: Some(chord.label().to_string()),
            clipboard_used: true,
            target: target.clone(),
            message: format!(
                "Pasted with {} into {}.",
                chord.label(),
                target.app_name.as_deref().or(target.app_id.as_deref()).unwrap_or("the focused window")
            ),
        },
        Err(err) => InjectReport {
            delivered: false,
            method: None,
            chord: Some(chord.label().to_string()),
            // The copy is attempted first, so the text is on the clipboard even
            // when the chord fails. Saying so is the difference between "your
            // words are gone" and "press paste yourself".
            clipboard_used: true,
            target: target.clone(),
            message: format!("On the clipboard — automatic paste failed: {err}"),
        },
    }
}

/// Type via uinput. ASCII only; see [`super::is_typeable`].
///
/// Text goes over stdin rather than as an argument: ydotool enables backslash
/// escape processing for arguments and disables it for stdin, and a transcript
/// containing a literal backslash must not be reinterpreted.
fn type_text(text: &str) -> Result<(), String> {
    if !command_exists("ydotool") {
        return Err("ydotool is not installed".to_string());
    }
    let mut child = ydotool()
        .args(["type", "--key-delay", "4", "--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "ydotool stdin unavailable".to_string())?
        .write_all(text.as_bytes())
        .map_err(|err| err.to_string())?;
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn paste_text(text: &str, chord: Chord) -> Result<(), String> {
    copy_to_clipboard(text)?;
    std::thread::sleep(CLIPBOARD_SETTLE);
    send_chord(chord)
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let candidates: Vec<(&str, Vec<&str>)> = if is_wayland() {
        vec![("wl-copy", vec![])]
    } else {
        vec![
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ]
    };
    for (program, args) in candidates {
        if !command_exists(program) {
            continue;
        }
        let mut child = match Command::new(program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => continue,
        };
        if let Some(stdin) = child.stdin.as_mut() {
            if stdin.write_all(text.as_bytes()).is_err() {
                continue;
            }
        }
        drop(child.stdin.take());
        // wl-copy forks and keeps running to own the selection, so waiting for
        // it to exit is correct only because it daemonises first.
        if child.wait().map(|status| status.success()).unwrap_or(false) {
            return Ok(());
        }
    }
    Err("no clipboard backend (install wl-clipboard, or xclip/xsel on X11)".to_string())
}

fn send_chord(chord: Chord) -> Result<(), String> {
    if is_wayland() {
        if !command_exists("ydotool") {
            return Err("ydotool is not installed".to_string());
        }
        // key syntax is `<keycode>:<1 press | 0 release>`, in order.
        let sequence: Vec<String> = match chord {
            Chord::Primary => vec![
                format!("{KEY_LEFTCTRL}:1"),
                format!("{KEY_V}:1"),
                format!("{KEY_V}:0"),
                format!("{KEY_LEFTCTRL}:0"),
            ],
            Chord::TerminalShift => vec![
                format!("{KEY_LEFTCTRL}:1"),
                format!("{KEY_LEFTSHIFT}:1"),
                format!("{KEY_V}:1"),
                format!("{KEY_V}:0"),
                format!("{KEY_LEFTSHIFT}:0"),
                format!("{KEY_LEFTCTRL}:0"),
            ],
        };
        let output = ydotool()
            .arg("key")
            .args(&sequence)
            .output()
            .map_err(|err| err.to_string())?;
        return if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        };
    }

    if !command_exists("xdotool") {
        return Err("xdotool is not installed".to_string());
    }
    let keys = match chord {
        Chord::Primary => "ctrl+v",
        Chord::TerminalShift => "ctrl+shift+v",
    };
    let output = Command::new("xdotool")
        .args(["key", "--clearmodifiers", keys])
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
