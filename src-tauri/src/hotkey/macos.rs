//! Push-to-talk on macOS via a `CGEventTap`.
//!
//! `NSEvent.addGlobalMonitorForEvents` would also deliver both key edges, but
//! it needs an active main-thread run loop, and taking over Tauri's main run
//! loop to poll for a hotkey is intrusive. A `CGEventTap` can own a
//! `CFRunLoop` on a thread of its own, so the listener stays self-contained.
//!
//! Carbon's `RegisterEventHotKey` — what Tauri's global-shortcut plugin uses —
//! is not an option at all here: it only ever fires on press.
//!
//! Requires Accessibility permission (System Settings > Privacy & Security >
//! Accessibility). Without it `CGEventTapCreate` returns null, which is
//! reported to the user rather than failing silently.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventType,
};

use super::{HotkeyBackend, HotkeyStatus, PttEdge};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeDictionaryKeyCallBacks: std::ffi::c_void;
    static kCFTypeDictionaryValueCallBacks: std::ffi::c_void;
    static kCFBooleanTrue: *const std::ffi::c_void;
    fn CFDictionaryCreate(
        allocator: *const std::ffi::c_void,
        keys: *const *const std::ffi::c_void,
        values: *const *const std::ffi::c_void,
        num: isize,
        key_cbs: *const std::ffi::c_void,
        value_cbs: *const std::ffi::c_void,
    ) -> *const std::ffi::c_void;
    fn CFRelease(cf: *const std::ffi::c_void);
    fn CFStringCreateWithCString(
        allocator: *const std::ffi::c_void,
        cstr: *const std::os::raw::c_char,
        encoding: u32,
    ) -> *const std::ffi::c_void;
}

/// Whether the app currently holds Accessibility permission.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Ask for Accessibility permission, showing the system prompt if it has not
/// been answered yet.
///
/// Called at startup rather than on first use: without this permission the
/// hotkey does nothing at all, and discovering that by holding a key and
/// getting silence is a bad first run. The system shows its prompt only once
/// ever — after that the user has to go to System Settings, which is why the
/// UI also has to surface the state rather than relying on this alone.
pub fn request_permission() -> bool {
    unsafe {
        if AXIsProcessTrusted() {
            return true;
        }
        // kAXTrustedCheckOptionPrompt; built by name because the symbol is not
        // exported in a form Rust can link to directly.
        let key_name = std::ffi::CString::new("AXTrustedCheckOptionPrompt").unwrap();
        // 0x0600 = kCFStringEncodingUTF8
        let key = CFStringCreateWithCString(std::ptr::null(), key_name.as_ptr(), 0x0800_0100);
        if key.is_null() {
            return false;
        }
        let keys = [key];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _,
            &kCFTypeDictionaryValueCallBacks as *const _,
        );
        let trusted = if options.is_null() {
            AXIsProcessTrusted()
        } else {
            let result = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            result
        };
        CFRelease(key);
        trusted
    }
}

/// macOS virtual key codes are a fixed hardware-derived layout, not ASCII, so
/// the letters need a table. Values from `<Carbon/Events.h>` (`kVK_ANSI_*`).
fn key_code_for(name: &str) -> Option<u16> {
    Some(match name {
        "SPACE" => 49,
        "A" => 0,
        "B" => 11,
        "C" => 8,
        "D" => 2,
        "E" => 14,
        "F" => 3,
        "G" => 5,
        "H" => 4,
        "I" => 34,
        "J" => 38,
        "K" => 40,
        "L" => 37,
        "M" => 46,
        "N" => 45,
        "O" => 31,
        "P" => 35,
        "Q" => 12,
        "R" => 15,
        "S" => 1,
        "T" => 17,
        "U" => 32,
        "V" => 9,
        "W" => 13,
        "X" => 7,
        "Y" => 16,
        "Z" => 6,
        _ => return None,
    })
}

/// Parse a portal-style trigger ("CTRL+ALT+space") into the modifier mask that
/// must be present and the virtual key code of the main key.
fn parse_trigger(trigger: &str) -> Result<(CGEventFlags, u16), String> {
    let mut mask = CGEventFlags::empty();
    let mut main: Option<u16> = None;

    for part in trigger.split('+') {
        let part = part.trim().to_ascii_uppercase();
        match part.as_str() {
            "CTRL" | "CONTROL" => mask |= CGEventFlags::CGEventFlagControl,
            "ALT" | "OPTION" => mask |= CGEventFlags::CGEventFlagAlternate,
            "SHIFT" => mask |= CGEventFlags::CGEventFlagShift,
            // Treat SUPER/META as Command, which is what a Linux-authored
            // binding means on a Mac keyboard.
            "SUPER" | "META" | "LOGO" | "CMD" | "COMMAND" => {
                mask |= CGEventFlags::CGEventFlagCommand
            }
            other => {
                main = Some(
                    key_code_for(other)
                        .ok_or_else(|| format!("unsupported key '{other}' in trigger"))?,
                )
            }
        }
    }

    let main = main.ok_or_else(|| format!("no main key in trigger '{trigger}'"))?;
    Ok((mask, main))
}

/// Modifier bits that matter for chord matching. Ignoring the rest keeps
/// Caps Lock, the numeric-keypad bit and left/right device bits from
/// preventing a match.
fn relevant(flags: CGEventFlags) -> CGEventFlags {
    flags
        & (CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagAlternate
            | CGEventFlags::CGEventFlagShift
            | CGEventFlags::CGEventFlagCommand)
}

pub fn start<F>(
    trigger: &str,
    stop: Arc<AtomicBool>,
    on_edge: Arc<F>,
) -> Result<(HotkeyStatus, JoinHandle<()>), String>
where
    F: Fn(PttEdge) + Send + Sync + 'static,
{
    let (want_mask, main_code) = parse_trigger(trigger)?;

    if !unsafe { AXIsProcessTrusted() } {
        return Err(
            "Accessibility permission is required. Grant it in System Settings > Privacy & \
             Security > Accessibility, then start push-to-talk again."
                .to_string(),
        );
    }

    let trigger_owned = trigger.to_string();
    let join = std::thread::spawn(move || {
        // The tap callback is an `Fn`, not an `FnMut`, so chord state has to
        // live behind shared interior mutability rather than being captured
        // by value.
        let engaged = Arc::new(AtomicBool::new(false));
        let engaged_cb = Arc::clone(&engaged);

        // ListenOnly: observe the key without swallowing it, so the chord
        // still reaches whatever app is focused.
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
            ],
            move |_proxy, event_type, event: &CGEvent| {
                let was = engaged_cb.load(Ordering::SeqCst);
                let code = event
                    .get_integer_value_field(core_graphics::event::EventField::KEYBOARD_EVENT_KEYCODE)
                    as u16;
                let mods = relevant(event.get_flags());

                // Reading modifiers from the event's own flags avoids having
                // to track their press/release separately; FlagsChanged then
                // only matters for releasing the chord by lifting a modifier
                // while the main key is still held.
                let now = match event_type {
                    CGEventType::KeyDown => mods.contains(want_mask) && code == main_code,
                    CGEventType::KeyUp => {
                        if code == main_code {
                            false
                        } else {
                            return None;
                        }
                    }
                    CGEventType::FlagsChanged => was && mods.contains(want_mask),
                    _ => return None,
                };

                if now && !was {
                    engaged_cb.store(true, Ordering::SeqCst);
                    on_edge(PttEdge::Pressed);
                } else if !now && was {
                    engaged_cb.store(false, Ordering::SeqCst);
                    on_edge(PttEdge::Released);
                }
                None
            },
        );

        let Ok(tap) = tap else {
            // Permission can be revoked between the check above and here.
            return;
        };

        let loop_source = match tap.mach_port.create_runloop_source(0) {
            Ok(source) => source,
            Err(_) => return,
        };
        let run_loop = CFRunLoop::get_current();
        unsafe {
            run_loop.add_source(&loop_source, kCFRunLoopCommonModes);
        }
        tap.enable();

        // Run in short slices so `stop` is honoured promptly; CFRunLoopRun()
        // would block until something explicitly stopped it.
        //
        // The mode here must be a concrete one. kCFRunLoopCommonModes is a
        // placeholder naming a *set* of modes: it is valid for add_source
        // above, but CFRunLoopRunInMode rejects it with "invalid mode ...
        // provided to CFRunLoopRunSpecific" and returns immediately, which
        // leaves the tap installed but never serviced.
        while !stop.load(Ordering::SeqCst) {
            CFRunLoop::run_in_mode(
                unsafe { kCFRunLoopDefaultMode },
                std::time::Duration::from_millis(250),
                false,
            );
        }
    });

    Ok((
        HotkeyStatus {
            backend: HotkeyBackend::NsEvent,
            trigger: trigger_owned,
            detail: "Listening through a CGEventTap. Keys are observed, not consumed, so the \
                     shortcut still reaches the focused app."
                .to_string(),
        },
        join,
    ))
}
