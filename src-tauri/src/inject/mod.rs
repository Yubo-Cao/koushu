//! Put transcribed text where the user was typing, without them pasting it.
//!
//! # Why this is not just "send Ctrl+V"
//!
//! Three things have to be true before text can be delivered, and each one
//! fails differently:
//!
//! 1. **We must know where it is going.** The voice bar is a layer-shell
//!    surface that never takes focus, so the application the user was in is
//!    still focused — but only the compositor knows which one it is. The target
//!    is captured *once*, when the utterance starts, not at delivery time: by
//!    then the user may have alt-tabbed, and delivering into whatever is
//!    focused at that moment is how dictation ends up in the wrong window.
//!
//! 2. **The text has to survive the transport.** Synthetic typing cannot carry
//!    CJK on this stack. Measured on this machine: `ydotool type` delivered
//!    `ascii-ok  mixed ` for the input `ascii-ok 中文测试 mixed 混排` — every
//!    Han character silently dropped. A uinput device types *keycodes*, and
//!    there is no keycode for 中. So anything outside the keymap must go
//!    through the clipboard.
//!
//! 3. **The paste chord is per-application.** See [`apps`].
//!
//! macOS escapes all of this: `CGEventKeyboardSetUnicodeString` posts the
//! characters themselves rather than keycodes, so arbitrary Unicode goes in
//! directly with no clipboard involvement and no chord to guess.
//!
//! # What this module will not do
//!
//! It never retracts text it has already delivered. Once characters are in
//! another application's document they belong to that application: the user may
//! have moved the caret, and synthesising backspaces to "correct" an earlier
//! segment would delete whatever is now in front of the cursor. Live delivery
//! is therefore append-only, and the accuracy consequences of that are the
//! caller's to decide — see `LiveMode` in the streaming worker.

pub mod apps;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

use serde::{Deserialize, Serialize};

/// Where text is headed, resolved once at the start of an utterance.
///
/// Deserialisable because the frontend captures it at push-to-talk time and
/// hands the same value back with each delivery, rather than the backend
/// re-resolving focus per segment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// Wayland `resourceClass` / X11 `WM_CLASS` / macOS bundle id.
    pub app_id: Option<String>,
    /// Window title or localised application name, for showing the user.
    pub app_name: Option<String>,
    pub pid: Option<u32>,
    /// Whether a text-accepting element actually holds focus.
    ///
    /// `None` means *the platform cannot tell*, which is the honest answer on
    /// Wayland: `zwp_text_input_v3` is a client-side protocol, and the only way
    /// to observe another application's text-input state is to become the input
    /// method — which would take the seat away from the user's real IME. On
    /// this machine that is fcitx5, and breaking Pinyin input to learn whether
    /// a text box is focused is not a trade worth making.
    ///
    /// `Some(false)` is a real negative and callers should refuse to deliver.
    pub accepts_text: Option<bool>,
}

impl Target {
    /// Whether delivering into this target is worth attempting.
    ///
    /// Unknown (`None`) counts as yes: refusing to deliver because we could not
    /// prove a text field is focused would disable the feature entirely on
    /// Wayland, where proof is unavailable by design.
    pub fn is_deliverable(&self) -> bool {
        self.accepts_text != Some(false)
    }
}

/// How the text actually got there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    /// Characters posted directly (macOS). No clipboard, any Unicode.
    Unicode,
    /// Synthesised keystrokes. Keymap-limited: ASCII only.
    Typed,
    /// Clipboard plus a paste chord.
    Pasted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectReport {
    pub delivered: bool,
    pub method: Option<Method>,
    /// The chord that was sent, when one was.
    pub chord: Option<String>,
    /// True when the clipboard was overwritten to carry the text.
    pub clipboard_used: bool,
    pub target: Target,
    /// Plain-language outcome, meant to be shown rather than logged.
    pub message: String,
}

impl InjectReport {
    fn failed(target: Target, message: impl Into<String>) -> Self {
        Self {
            delivered: false,
            method: None,
            chord: None,
            clipboard_used: false,
            target,
            message: message.into(),
        }
    }
}

/// Can this text be delivered as synthetic keystrokes?
///
/// A uinput keyboard emits keycodes from the active keymap, so only characters
/// that keymap contains can be typed. Restricting to printable ASCII is
/// deliberately conservative: Latin-1 accented characters are reachable on some
/// layouts and not others, and a character that silently vanishes is worse than
/// one that took the clipboard path.
pub fn is_typeable(text: &str) -> bool {
    text.chars()
        .all(|c| (' '..='~').contains(&c) || c == '\n' || c == '\t')
}

/// Resolve the focused application. Cheap enough to call per utterance, not
/// per segment.
pub fn capture_target() -> Target {
    #[cfg(target_os = "linux")]
    {
        linux::capture_target()
    }
    #[cfg(target_os = "macos")]
    {
        macos::capture_target()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Target::default()
    }
}

/// Deliver `text` into `target`.
///
/// `keep_clipboard` asks the backend to avoid the clipboard when it has a
/// choice. Live dictation sets it: overwriting the clipboard once per spoken
/// phrase would destroy whatever the user had copied, dozens of times per
/// minute. The final delivery leaves it false, so the completed transcript ends
/// up on the clipboard where the user can paste it again.
pub fn inject(text: &str, target: &Target, keep_clipboard: bool) -> InjectReport {
    if text.trim().is_empty() {
        return InjectReport::failed(target.clone(), "Nothing to insert.");
    }
    if !target.is_deliverable() {
        return InjectReport::failed(
            target.clone(),
            "The focused element does not accept text, so nothing was inserted.",
        );
    }

    #[cfg(target_os = "linux")]
    {
        linux::inject(text, target, keep_clipboard)
    }
    #[cfg(target_os = "macos")]
    {
        macos::inject(text, target, keep_clipboard)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = keep_clipboard;
        InjectReport::failed(
            target.clone(),
            "Text insertion is not implemented on this platform.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_is_never_considered_typeable() {
        // This is the measured constraint the whole clipboard path exists for:
        // ydotool dropped every Han character it was asked to type.
        assert!(!is_typeable("中文测试"));
        assert!(!is_typeable("ascii 中文 mixed"));
        assert!(!is_typeable("café"));
    }

    #[test]
    fn plain_ascii_is_typeable() {
        assert!(is_typeable("hello, world! 123 (a+b)=c"));
        assert!(is_typeable("line one\nline two\ttabbed"));
    }

    #[test]
    fn an_unknown_focus_state_still_delivers() {
        // Wayland cannot prove a text field is focused. Treating that as "no"
        // would disable the feature on the platform it was built for.
        let unknown = Target::default();
        assert!(unknown.is_deliverable());
    }

    #[test]
    fn a_proven_non_text_target_refuses() {
        let target = Target {
            accepts_text: Some(false),
            ..Target::default()
        };
        assert!(!target.is_deliverable());
        let report = inject("hello", &target, false);
        assert!(!report.delivered);
    }

    #[test]
    fn empty_text_is_not_delivered() {
        assert!(!inject("   ", &Target::default(), false).delivered);
    }
}
