//! macOS delivery: post the characters themselves.
//!
//! `CGEventKeyboardSetUnicodeString` attaches a UTF-16 payload to a keyboard
//! event instead of a keycode, so the receiving application gets the characters
//! regardless of the active layout. That removes, in one call, both problems
//! Linux has: no clipboard is touched, and there is no paste chord to guess —
//! terminals accept it exactly like text fields do.
//!
//! The Accessibility API additionally answers the question Wayland cannot: it
//! reports the *role* of the focused element, so a target that genuinely cannot
//! take text is refused rather than typed into.

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

use super::{InjectReport, Method, Target};

type AXUIElementRef = CFTypeRef;
type AXError = i32;
const AX_SUCCESS: AXError = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
}

/// Roles whose elements take typed text.
///
/// Deliberately a list of yes-roles rather than a list of no-roles: an
/// unrecognised role returns "unknown", which still delivers. Guessing "no" for
/// anything unfamiliar would silently disable dictation in every application
/// that uses a custom control — which, on this platform, is most of the
/// Electron ones.
const TEXT_ROLES: &[&str] = &[
    "AXTextField",
    "AXTextArea",
    "AXComboBox",
    "AXSearchField",
    "AXSecureTextField",
];

/// Roles that are definitely not text. Only these produce `Some(false)`.
const NON_TEXT_ROLES: &[&str] = &["AXButton", "AXCheckBox", "AXRadioButton", "AXSlider", "AXMenuItem"];

fn copy_attribute(element: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    if element.is_null() {
        return None;
    }
    let attribute = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    if status == AX_SUCCESS && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

pub fn capture_target() -> Target {
    let mut target = Target::default();

    // Bundle id of whatever is frontmost. Unlike the focused-element query this
    // never needs Accessibility permission, so the target is still named in the
    // UI even when permission has not been granted yet.
    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if !workspace.is_null() {
            let app: *mut Object = msg_send![workspace, frontmostApplication];
            if !app.is_null() {
                let bundle: *mut Object = msg_send![app, bundleIdentifier];
                if !bundle.is_null() {
                    let utf8: *const std::os::raw::c_char = msg_send![bundle, UTF8String];
                    if !utf8.is_null() {
                        target.app_id = std::ffi::CStr::from_ptr(utf8)
                            .to_str()
                            .ok()
                            .map(str::to_string);
                    }
                }
                let name: *mut Object = msg_send![app, localizedName];
                if !name.is_null() {
                    let utf8: *const std::os::raw::c_char = msg_send![name, UTF8String];
                    if !utf8.is_null() {
                        target.app_name = std::ffi::CStr::from_ptr(utf8)
                            .to_str()
                            .ok()
                            .map(str::to_string);
                    }
                }
                let pid: i32 = msg_send![app, processIdentifier];
                if pid > 0 {
                    target.pid = Some(pid as u32);
                }
            }
        }
    }

    let system = unsafe { AXUIElementCreateSystemWide() };
    if let Some(focused) = copy_attribute(system, "AXFocusedUIElement") {
        if let Some(role_ref) = copy_attribute(focused, "AXRole") {
            let role = unsafe { CFString::wrap_under_get_rule(role_ref as CFStringRef) }.to_string();
            target.accepts_text = if TEXT_ROLES.contains(&role.as_str()) {
                Some(true)
            } else if NON_TEXT_ROLES.contains(&role.as_str()) {
                Some(false)
            } else {
                None
            };
            unsafe { CFRelease(role_ref) };
        }
        unsafe { CFRelease(focused) };
    }
    if !system.is_null() {
        unsafe { CFRelease(system) };
    }

    target
}

/// `CGEventKeyboardSetUnicodeString` is documented to carry a short string, and
/// long payloads are truncated in practice, so text is posted in small chunks.
/// Chunking is over UTF-16 code units and never splits a surrogate pair — doing
/// so would deliver two replacement characters instead of one emoji.
const CHUNK_UTF16_UNITS: usize = 16;

fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut units = 0usize;
    for ch in text.chars() {
        let width = ch.len_utf16();
        if units + width > CHUNK_UTF16_UNITS && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            units = 0;
        }
        current.push(ch);
        units += width;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn inject(text: &str, target: &Target, keep_clipboard: bool) -> InjectReport {
    // keep_clipboard is irrelevant here: this path never uses the clipboard, so
    // the caller's preference is already satisfied either way.
    let _ = keep_clipboard;

    let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return InjectReport {
            delivered: false,
            method: None,
            chord: None,
            clipboard_used: false,
            target: target.clone(),
            message: "Could not create an event source; check Accessibility permission."
                .to_string(),
        };
    };

    for chunk in chunks(text) {
        // Down then up: some applications act on key-up, and posting only the
        // down event leaves them believing a key is still held.
        for pressed in [true, false] {
            let Ok(event) = CGEvent::new_keyboard_event(source.clone(), 0, pressed) else {
                return InjectReport {
                    delivered: false,
                    method: None,
                    chord: None,
                    clipboard_used: false,
                    target: target.clone(),
                    message: "Could not create a keyboard event.".to_string(),
                };
            };
            event.set_string(&chunk);
            event.post(CGEventTapLocation::HID);
        }
    }

    InjectReport {
        delivered: true,
        method: Some(Method::Unicode),
        chord: None,
        clipboard_used: false,
        target: target.clone(),
        message: format!(
            "Inserted into {}.",
            target
                .app_name
                .as_deref()
                .or(target.app_id.as_deref())
                .unwrap_or("the focused application")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking_never_splits_a_surrogate_pair() {
        let text = "😀".repeat(20);
        let parts = chunks(&text);
        for part in &parts {
            assert!(part.chars().all(|c| c == '😀'));
        }
        assert_eq!(parts.concat(), text);
    }

    #[test]
    fn chunking_preserves_mixed_text_exactly() {
        let text = "hello 中文 world 混排 1234567890";
        assert_eq!(chunks(text).concat(), text);
    }
}
