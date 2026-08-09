//! The system tray icon, on Linux and macOS.
//!
//! This application is driven almost entirely by a global hotkey. The main
//! window spends most of its life closed, nothing sits in the taskbar, and the
//! voice bar only appears while you are actually talking — so without a tray
//! icon there is no way to tell a running instance from a crashed one. That is
//! the icon's first job: proof of life.
//!
//! Its second job is state. An icon that never changes proves the process
//! exists but says nothing about what it is doing, and the two things the user
//! cannot otherwise see are *the microphone is open* and *your words are still
//! being transcribed*. So there are three icons, not one:
//!
//! | State | Glyph | Meaning |
//! |---|---|---|
//! | Idle | outlined mic | running, not listening |
//! | Recording | filled mic | the microphone is open |
//! | Transcribing | waveform | audio is being turned into text |
//!
//! They differ by shape rather than only by colour, which matters on macOS:
//! a template image is rendered from its alpha channel alone, so colour is not
//! information the platform will keep.
//!
//! ## How the state is observed
//!
//! By polling `AppState`, not by call-backs threaded through every command.
//! The recording flag already exists there (`audio_capture` is `Some` exactly
//! while the microphone is open) and transcription jobs are counted by an RAII
//! guard. Polling costs two uncontended mutex loads every 350 ms and keeps this
//! module a leaf: nothing else in the codebase has to know the tray exists.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{include_image, AppHandle, Manager, WindowEvent};

use crate::AppState;

/// Icons are 64x64 and downscaled by both platforms; see `icons/tray/generate.py`
/// for why that size and not the nominal 22px.
const ICON_IDLE: Image<'_> = include_image!("icons/tray/idle.png");
const ICON_IDLE_TEMPLATE: Image<'_> = include_image!("icons/tray/idle-template.png");
const ICON_RECORDING: Image<'_> = include_image!("icons/tray/recording.png");
const ICON_TRANSCRIBING: Image<'_> = include_image!("icons/tray/transcribing.png");

/// Transcription jobs currently in flight.
///
/// A count rather than a flag: several can overlap when a long dictation is
/// segmented, and a flag would clear on the first one that finished.
static ASR_JOBS: AtomicUsize = AtomicUsize::new(0);

/// Marks a transcription job as running for as long as it is alive.
///
/// An RAII guard rather than a pair of calls because the job can end by `?`,
/// by panic, or by the future being dropped, and only a destructor covers all
/// three. A leaked increment would pin the tray to "transcribing" forever.
pub struct AsrJobGuard;

impl AsrJobGuard {
    pub fn new() -> Self {
        ASR_JOBS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for AsrJobGuard {
    fn drop(&mut self) {
        ASR_JOBS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Activity {
    Idle,
    Recording,
    Transcribing,
}

impl Activity {
    fn icon(self) -> Image<'static> {
        match self {
            // The idle icon is the one that is on screen ~all the time, so on
            // macOS it is a template image: the platform then tints it to match
            // the menu bar, light or dark, and it never looks like a sticker.
            // The active states give that up on purpose — red and amber are the
            // whole point of them, and both read on either menu bar.
            Self::Idle if cfg!(target_os = "macos") => ICON_IDLE_TEMPLATE,
            Self::Idle => ICON_IDLE,
            Self::Recording => ICON_RECORDING,
            Self::Transcribing => ICON_TRANSCRIBING,
        }
    }

    fn is_template(self) -> bool {
        cfg!(target_os = "macos") && matches!(self, Self::Idle)
    }
}

/// The UI language, mirrored from the same `ui.locale` setting the webviews use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Locale {
    En,
    Zh,
}

impl Locale {
    fn status(self, activity: Activity) -> &'static str {
        match (self, activity) {
            (Self::Zh, Activity::Idle) => "空闲",
            (Self::Zh, Activity::Recording) => "正在录音…",
            (Self::Zh, Activity::Transcribing) => "正在转写…",
            (Self::En, Activity::Idle) => "Idle",
            (Self::En, Activity::Recording) => "Recording…",
            (Self::En, Activity::Transcribing) => "Transcribing…",
        }
    }

    fn open(self) -> &'static str {
        match self {
            Self::Zh => "打开主窗口",
            Self::En => "Open Fun ASR",
        }
    }

    fn settings(self) -> &'static str {
        match self {
            Self::Zh => "设置…",
            Self::En => "Settings…",
        }
    }

    fn quit(self) -> &'static str {
        match self {
            Self::Zh => "退出 Fun ASR",
            Self::En => "Quit Fun ASR",
        }
    }
}

/// Read the locale the user actually picked, falling back to the environment.
///
/// `ui.locale` is written by the language switcher, so the tray follows the
/// same choice as the rest of the UI instead of having its own idea.
fn current_locale(app: &AppHandle) -> Locale {
    let from_setting = app
        .try_state::<AppState>()
        .and_then(|state| crate::setting_value(&state, "ui.locale").ok().flatten());
    match from_setting.as_deref() {
        Some("zh") => return Locale::Zh,
        Some("en") => return Locale::En,
        _ => {}
    }
    // No stored choice yet — first run, so both sides have to guess, and they
    // have to guess the same way or the tray ends up in English under a Chinese
    // interface. The frontend reads `navigator.language`, which is the *system*
    // language; matching that means asking the system, not the environment.
    if system_language().starts_with("zh") {
        Locale::Zh
    } else {
        Locale::En
    }
}

/// The system's preferred language tag, lowercased.
///
/// Per platform, because the environment is not the answer everywhere. On Linux
/// the locale variables are the system's own configuration. On macOS they are
/// usually unset for a GUI app — an `.app` launched from Finder inherits almost
/// nothing — so reading them there silently answered "English" on a Chinese Mac,
/// which is exactly the mismatch this function exists to avoid.
fn system_language() -> String {
    #[cfg(target_os = "macos")]
    {
        // `AppleLanguages` is an ordered preference list; the first entry is
        // what the user actually reads the system in.
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = text
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with('"'))
            {
                return first.trim_matches(['"', ',', ' '].as_ref()).to_ascii_lowercase();
            }
        }
    }

    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Everything the watcher thread needs to keep in step with the app.
///
/// The watcher *owns* these rather than borrowing them through a static: a
/// `TrayIcon` is reference counted and the icon disappears when the last
/// handle drops, so the thread that outlives `setup()` is exactly the right
/// place to keep them alive.
struct TrayHandles {
    tray: tauri::tray::TrayIcon,
    status: MenuItem<tauri::Wry>,
    open: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

/// Build the tray icon and start watching the app's state.
///
/// Call once from `setup`. Failure is reported, never fatal: a missing tray is
/// a degraded experience, but refusing to start the whole application because
/// no StatusNotifier host is running would be worse.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let locale = current_locale(app);
    let activity = Activity::Idle;

    // The status line is a disabled item, which is how a menu says "this is a
    // label" — a click target that did nothing would just look broken.
    let status = MenuItem::with_id(app, "tray-status", locale.status(activity), false, None::<&str>)
        .map_err(|err| err.to_string())?;
    let open = MenuItem::with_id(app, "tray-open", locale.open(), true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let settings = MenuItem::with_id(app, "tray-settings", locale.settings(), true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let quit = MenuItem::with_id(app, "tray-quit", locale.quit(), true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let sep_top = PredefinedMenuItem::separator(app).map_err(|err| err.to_string())?;
    let sep_bottom = PredefinedMenuItem::separator(app).map_err(|err| err.to_string())?;

    let menu = Menu::with_items(
        app,
        &[&status, &sep_top, &open, &settings, &sep_bottom, &quit],
    )
    .map_err(|err| err.to_string())?;

    let tray = TrayIconBuilder::with_id("fun-asr-tray")
        .icon(activity.icon())
        .icon_as_template(activity.is_template())
        .tooltip("Fun ASR Desktop")
        .menu(&menu)
        // Linux ignores this (libappindicator only ever opens the menu), but on
        // macOS it keeps one gesture doing one thing on both platforms.
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
        .build(app)
        .map_err(|err| err.to_string())?;

    keep_alive_on_close(app);
    spawn_watcher(
        app.clone(),
        TrayHandles {
            tray,
            status,
            open,
            settings,
            quit,
        },
        locale,
    );
    Ok(())
}

/// Closing the main window hides it instead of ending the process.
///
/// Without this the tray icon would be self-defeating: the window is the only
/// one that keeps the app alive on Linux, so closing it would take the very
/// icon that is meant to prove the app is still running with it. Quitting is
/// still one click away, in the tray menu, which is where an app that lives in
/// the tray is expected to put it.
fn keep_alive_on_close(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let hidden = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hidden.hide();
        }
    });
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "tray-open" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "tray-settings" => {
            if let Err(err) = crate::show_settings_window(app.clone()) {
                eprintln!("[tray] could not open settings: {err}");
            }
        }
        "tray-quit" => app.exit(0),
        _ => {}
    }
}

/// Poll the app's state and keep the icon and menu in step with it.
///
/// The polling runs on its own thread, but **every call that touches the tray
/// or the menu is marshalled back onto the main thread**. On Linux those go
/// through libappindicator and therefore through GTK, which is not thread-safe:
/// calling in from here aborted the process whenever the main thread happened
/// to be doing GTK work of its own. That made it look like three unrelated
/// bugs — resizing a window, pressing the hotkey, and dictating into a terminal
/// each "crashed the app" — when the only thing they had in common was being
/// busy at the moment the watcher touched the icon.
///
/// Note this is why the handles live behind an `Arc`: the thread keeps one
/// clone so the tray outlives `setup()` (a `TrayIcon` is reference counted and
/// the icon vanishes with its last handle), and each main-thread closure needs
/// its own.
fn spawn_watcher(app: AppHandle, handles: TrayHandles, initial_locale: Locale) {
    let handles = std::sync::Arc::new(handles);
    std::thread::spawn(move || {
        let mut shown_activity = Activity::Idle;
        let mut shown_locale = initial_locale;
        loop {
            std::thread::sleep(Duration::from_millis(350));

            let activity = observe(&app, shown_activity);
            let locale = current_locale(&app);
            if activity == shown_activity && locale == shown_locale {
                continue;
            }
            let icon_changed = activity != shown_activity;
            let locale_changed = locale != shown_locale;
            shown_activity = activity;
            shown_locale = locale;

            let handles = handles.clone();
            let result = app.run_on_main_thread(move || {
                if icon_changed {
                    if let Err(err) = handles
                        .tray
                        .set_icon_with_as_template(Some(activity.icon()), activity.is_template())
                    {
                        eprintln!("[tray] could not swap the icon: {err}");
                    }
                }
                if locale_changed {
                    let _ = handles.open.set_text(locale.open());
                    let _ = handles.settings.set_text(locale.settings());
                    let _ = handles.quit.set_text(locale.quit());
                }
                let _ = handles.status.set_text(locale.status(activity));
            });
            // The main thread is gone only when the app is shutting down, in
            // which case so should this.
            if result.is_err() {
                return;
            }
        }
    });
}

/// What the app is doing right now.
///
/// Recording outranks transcribing when both are true — a live preview runs
/// alongside the microphone, and of the two facts "your microphone is open" is
/// the one the user needs to be able to see at a glance.
///
/// `try_lock` rather than `lock`: this runs on a timer and blocking here would
/// park the watcher behind whichever command holds the lock. But a *failed*
/// sample is evidence of nothing, and must not be read as "idle" — the level
/// meter takes that same lock several times a second for the entire time the
/// microphone is open, which is precisely when contention is likely and
/// precisely when saying "idle" would be a lie. So a contended sample keeps
/// the previous answer, and the icon holds still instead of strobing.
fn observe(app: &AppHandle, previous: Activity) -> Activity {
    if let Some(state) = app.try_state::<AppState>() {
        match state.audio_capture.try_lock() {
            Ok(capture) => {
                if capture.is_some() {
                    return Activity::Recording;
                }
            }
            Err(_) => return previous,
        }
    }
    if ASR_JOBS.load(Ordering::SeqCst) > 0 {
        return Activity::Transcribing;
    }
    Activity::Idle
}
