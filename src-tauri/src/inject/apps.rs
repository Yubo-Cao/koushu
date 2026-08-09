//! Which key chord pastes, per application.
//!
//! `Ctrl+V` is not universal. Terminal emulators moved paste to `Ctrl+Shift+V`
//! because `Ctrl+V` was already taken: in readline it is `quoted-insert`.
//!
//! Sending the wrong chord is worse than sending none. Measured against a real
//! Konsole running `read -r line`: with `Ctrl+Shift+V` the line arrived intact
//! (`终端里的中文 terminal-ok`); with `Ctrl+V` nothing was pasted *and* the
//! following Return was swallowed as the quoted character, so `read` never
//! returned at all. A wrong chord does not fail quietly — it eats the user's
//! next keystroke.
//!
//! This table is deliberately a list of *known* applications rather than a
//! guess. An unknown application gets `Ctrl+V`, which is right far more often
//! than not, and the caller keeps the text on the clipboard either way so a
//! wrong guess costs one manual paste rather than the transcript.

/// A paste chord, expressed once and lowered per platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// The ordinary desktop paste.
    Primary,
    /// Terminal paste: the same key with Shift added.
    TerminalShift,
}

impl Chord {
    /// Human-readable name, for the status line and for reports. Users debug
    /// "it pasted the wrong thing" by knowing which chord was sent.
    pub fn label(self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Chord::Primary => "Cmd+V",
            #[cfg(target_os = "macos")]
            Chord::TerminalShift => "Cmd+V",
            #[cfg(not(target_os = "macos"))]
            Chord::Primary => "Ctrl+V",
            #[cfg(not(target_os = "macos"))]
            Chord::TerminalShift => "Ctrl+Shift+V",
        }
    }
}

/// Applications whose paste is `Ctrl+Shift+V`.
///
/// Matched against the Wayland `resourceClass` / X11 `WM_CLASS`, lowercased.
/// Reverse-DNS ids are matched on their last component too, since KWin reports
/// `org.kde.konsole` for some builds and `konsole` for others.
const TERMINALS: &[&str] = &[
    "konsole",
    "alacritty",
    "kitty",
    "foot",
    "footclient",
    "ghostty",
    "wezterm",
    "wezterm-gui",
    "gnome-terminal",
    "gnome-terminal-server",
    "terminator",
    "tilix",
    "ptyxis",
    "blackbox",
    "black-box",
    "contour",
    "xterm",
    "uxterm",
    "urxvt",
    "rxvt",
    "st",
    "qterminal",
    "deepin-terminal",
    "xfce4-terminal",
    "lxterminal",
    "mate-terminal",
    "terminology",
    "guake",
    "yakuake",
    "tilda",
    "hyper",
    "warp",
    "wave",
];

/// On macOS the terminals use the ordinary Cmd+V, so this list only exists to
/// keep the Linux behaviour honest; it is consulted on every platform but only
/// changes the answer where the chord actually differs.
pub fn chord_for(app_id: Option<&str>) -> Chord {
    let Some(raw) = app_id else {
        return Chord::Primary;
    };
    let lower = raw.to_ascii_lowercase();
    let tail = lower.rsplit('.').next().unwrap_or(&lower);
    if TERMINALS.contains(&lower.as_str()) || TERMINALS.contains(&tail) {
        Chord::TerminalShift
    } else {
        Chord::Primary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_get_the_shifted_chord() {
        for id in ["konsole", "Alacritty", "org.kde.konsole", "kitty", "ghostty"] {
            assert_eq!(chord_for(Some(id)), Chord::TerminalShift, "{id}");
        }
    }

    #[test]
    fn everything_else_gets_the_ordinary_paste() {
        for id in ["firefox", "org.kde.kdialog", "code", "WeChat", "Slack"] {
            assert_eq!(chord_for(Some(id)), Chord::Primary, "{id}");
        }
    }

    #[test]
    fn an_unknown_target_is_not_treated_as_a_terminal() {
        // Guessing "terminal" for an unknown app would send Ctrl+Shift+V into
        // an ordinary text field, where it usually does nothing at all.
        assert_eq!(chord_for(None), Chord::Primary);
    }

    #[test]
    fn a_reverse_dns_id_matches_on_its_last_component() {
        assert_eq!(chord_for(Some("org.wezfurlong.wezterm")), Chord::TerminalShift);
    }
}
