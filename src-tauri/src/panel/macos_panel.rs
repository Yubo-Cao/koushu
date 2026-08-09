//! Anchor the voice bar as a floating macOS panel, and give it real glass.
//!
//! macOS has no layer-shell, but `NSWindow` exposes the same properties
//! piecemeal: a status-bar window level puts it above ordinary windows, and
//! the right collection behaviour keeps it present on every Space and out of
//! the window cycle.
//!
//! Focus is the important part. `NSNonactivatingPanelMask` lets the window
//! show without activating the app, which is what makes "hold the hotkey while
//! another app is focused, then paste back into it" work at all.
//!
//! ## Why the material is native and not CSS
//!
//! `backdrop-filter` samples the page's own compositing result. Behind a
//! pill-sized window there is no page — the desktop belongs to the window
//! server — so CSS can only ever *paint a picture of* glass: a tint, a
//! gradient, a drawn highlight. That picture is what read as "光感太强": a
//! specular rim at a fixed brightness does not know whether it is sitting on a
//! white wallpaper or a black terminal, so it is too hot half the time and the
//! whole capsule reads as moulded plastic.
//!
//! `NSGlassEffectView` is the real thing: it refracts what is behind the
//! window, and its highlight and its shadow are derived from that content, so
//! it darkens over dark backdrops on its own. This module's job is to put that
//! view behind the webview and then get every opaque pixel out of its way.

use objc::runtime::{Object, BOOL, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::{AtomicPtr, Ordering};

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

unsafe impl objc::Encode for NSPoint {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
    }
}

unsafe impl objc::Encode for NSSize {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGSize=dd}") }
    }
}

unsafe impl objc::Encode for NSRect {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

use super::{PanelAnchor, PanelStatus};

/// `NSStatusWindowLevel`. Above normal and floating windows, below the screen
/// saver and alerts.
const NS_STATUS_WINDOW_LEVEL: i64 = 25;

// NSWindowCollectionBehavior bits.
const CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
const TRANSIENT: u64 = 1 << 3;
const IGNORES_CYCLE: u64 = 1 << 6;
const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

/// NSWindowStyleMask bit that lets a panel show without activating the app.
const NONACTIVATING_PANEL: u64 = 1 << 7;

/// `NSViewWidthSizable | NSViewHeightSizable`. With the view inset on all four
/// sides, keeping both dimensions sizable keeps the inset fixed as the window
/// grows, which is exactly what the capsule wants.
const AUTORESIZE_WIDTH_HEIGHT: u64 = 2 | 16;

/// `NSVisualEffectMaterial.hudWindow` — the pre-26 stand-in for glass on a
/// small floating panel.
const MATERIAL_HUD_WINDOW: i64 = 13;
/// `NSVisualEffectBlendingMode.behindWindow`: frost the desktop, not our own
/// content.
const BLENDING_BEHIND_WINDOW: i64 = 0;
/// `NSVisualEffectState.active`: stay lit while another app is frontmost,
/// which is the only state this bar is ever seen in.
const EFFECT_STATE_ACTIVE: i64 = 1;

/// `NSWindowOrderingMode.below`, for `addSubview:positioned:relativeTo:`.
///
/// **Minus one, not zero.** This constant was 0 here, annotated "0 =
/// NSWindowBelow"; 0 is `NSWindowOut`, which asks AppKit to *unplace* the view,
/// and AppKit answers that by aborting the process inside
/// `-[NSView addSubview:positioned:relativeTo:]`. It never showed up because
/// the only caller ran behind a permission the Mac had never been granted, so
/// the crashing line had never once executed.
const NS_WINDOW_BELOW: i64 = -1;

/// How far the pill sits inside its window, in points.
///
/// `app/bar/page.tsx` sizes this window to the measured pill plus 12 points —
/// six on each side. The glass has to be shaped like the *capsule*, not like
/// the window: give it the full bounds and the material shows up as a
/// rectangle with the pill floating somewhere inside it. So these two numbers
/// are one number, and changing the 12 there without changing the 6 here puts
/// the material out of register with the content it is supposed to be.
const PILL_INSET: f64 = 6.0;

/// The glass view, so a later resize can reshape it.
///
/// A raw pointer in a static rather than state threaded through `AppState`,
/// because the only two things that ever touch it — install and resize — both
/// already have to be on the main thread for AppKit's sake, and that, not the
/// storage, is what makes it safe. Every read below is inside a
/// `run_on_main_thread` closure for that reason.
static GLASS_VIEW: AtomicPtr<Object> = AtomicPtr::new(std::ptr::null_mut());

unsafe fn ns_string(value: &str) -> *mut Object {
    let c = std::ffi::CString::new(value).unwrap();
    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
}

unsafe fn responds(target: *mut Object, selector: objc::runtime::Sel) -> bool {
    let answer: BOOL = msg_send![target, respondsToSelector: selector];
    answer == YES
}

fn inset(rect: NSRect, by: f64) -> NSRect {
    NSRect {
        origin: NSPoint {
            x: rect.origin.x + by,
            y: rect.origin.y + by,
        },
        size: NSSize {
            width: (rect.size.width - by * 2.0).max(0.0),
            height: (rect.size.height - by * 2.0).max(0.0),
        },
    }
}

/// Stop the webview painting an opaque page backdrop.
///
/// This is the step without which none of the rest is visible. A `WKWebView`
/// fills its bounds with a base colour before the document paints — white, or
/// `#1E1E1E` once the page declares `color-scheme: dark` — and it sits above
/// anything added to the window's content view. A transparent `<body>` in CSS
/// does not reach it: the page is transparent *over that base*, so the window
/// still shows a dark rounded-corner-less rectangle around the capsule, and a
/// glass view underneath is a material nobody can see.
///
/// `drawsBackground` is set through KVC because WebKit exposes no public
/// setter for it on macOS. That is a deliberate trade and worth naming: it is
/// private API, so an App Store submission would have to fall back to the
/// painted material. Nothing else can produce a genuinely transparent webview,
/// and without one there is no way to show the system's own glass at all.
fn set_webview_transparent(window: &tauri::WebviewWindow) -> Result<(), String> {
    window
        .with_webview(|webview| unsafe {
            let view = webview.inner() as *mut Object;
            if view.is_null() {
                return;
            }
            let no: *mut Object = msg_send![class!(NSNumber), numberWithBool: NO];
            let key = ns_string("drawsBackground");
            let _: () = msg_send![view, setValue: no forKey: key];
        })
        .map_err(|err| err.to_string())
}

/// Build the backing view, preferring the real material when the OS has it.
///
/// macOS 26 added `NSGlassEffectView`, which refracts rather than blurs and
/// adapts its own brightness to what is behind the window. Earlier versions
/// get `NSVisualEffectView` in `behindWindow` mode, which frosts the desktop —
/// less alive, but the same idea and the same "the system owns the material"
/// contract.
unsafe fn make_glass_view(frame: NSRect) -> (*mut Object, &'static str) {
    let radius = frame.size.height / 2.0;

    if let Some(cls) = objc::runtime::Class::get("NSGlassEffectView") {
        let view: *mut Object = msg_send![cls, alloc];
        let view: *mut Object = msg_send![view, initWithFrame: frame];
        if responds(view, sel!(setCornerRadius:)) {
            let _: () = msg_send![view, setCornerRadius: radius];
        }
        // `.regular`, not `.clear`. Clear glass is for material laid over
        // video, where the content underneath supplies the contrast; over an
        // arbitrary desktop it leaves white-on-white text unreadable.
        if responds(view, sel!(setStyle:)) {
            let _: () = msg_send![view, setStyle: 0i64];
        }
        return (view, "NSGlassEffectView");
    }

    let cls = class!(NSVisualEffectView);
    let view: *mut Object = msg_send![cls, alloc];
    let view: *mut Object = msg_send![view, initWithFrame: frame];
    let _: () = msg_send![view, setBlendingMode: BLENDING_BEHIND_WINDOW];
    let _: () = msg_send![view, setState: EFFECT_STATE_ACTIVE];
    let _: () = msg_send![view, setMaterial: MATERIAL_HUD_WINDOW];
    // NSVisualEffectView has no corner radius of its own, so the capsule has
    // to be cut out of its layer.
    let _: () = msg_send![view, setWantsLayer: YES];
    let layer: *mut Object = msg_send![view, layer];
    if !layer.is_null() {
        let _: () = msg_send![layer, setCornerRadius: radius];
        let _: () = msg_send![layer, setMasksToBounds: YES];
    }
    (view, "NSVisualEffectView")
}

/// Install a real glass backing behind the webview.
///
/// Call once, from `setup`, on the main thread. It used to be called lazily
/// from the first push-to-talk show, which meant the bar opened from the
/// toolbar button had no material at all and the hotkey path only got one
/// after the first recording.
pub fn install_glass(window: &tauri::WebviewWindow) -> Result<&'static str, String> {
    let ns_window = window.ns_window().map_err(|err| err.to_string())? as *mut Object;
    if ns_window.is_null() {
        return Err("window has no NSWindow yet".to_string());
    }

    set_webview_transparent(window)?;

    unsafe {
        let _: () = msg_send![ns_window, setOpaque: NO];
        let clear: *mut Object = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![ns_window, setBackgroundColor: clear];

        // The window is shaped by its alpha now, and macOS will fit a shadow
        // to that shape — which is where the bar's sense of thickness comes
        // from. The CSS could not do this: a `box-shadow` has to fall inside
        // the window, and this window is cut to the capsule.
        let _: () = msg_send![ns_window, setHasShadow: YES];

        let content: *mut Object = msg_send![ns_window, contentView];
        if content.is_null() {
            return Err("window has no content view".to_string());
        }
        let bounds: NSRect = msg_send![content, bounds];
        let (view, kind) = make_glass_view(inset(bounds, PILL_INSET));

        let _: () = msg_send![view, setAutoresizingMask: AUTORESIZE_WIDTH_HEIGHT];
        // Behind the webview rather than over it.
        let _: () = msg_send![content, addSubview: view positioned: NS_WINDOW_BELOW relativeTo: std::ptr::null::<Object>()];
        let _: () = msg_send![ns_window, invalidateShadow];

        GLASS_VIEW.store(view, Ordering::SeqCst);
        Ok(kind)
    }
}

/// Reshape the glass after the bar has changed size.
///
/// The capsule's radius is half its height, and the height changes with the
/// bar's state — idle is a stub, transcribing is a wide strip. Autoresizing
/// keeps the frame right on its own; only the radius and the window's shadow
/// have to be recomputed, and both have to happen on the main thread.
pub fn sync_glass(window: &tauri::WebviewWindow) {
    let window = window.clone();
    let _ = window.clone().run_on_main_thread(move || unsafe {
        let view = GLASS_VIEW.load(Ordering::SeqCst);
        if view.is_null() {
            return;
        }
        let superview: *mut Object = msg_send![view, superview];
        if superview.is_null() {
            return;
        }
        let bounds: NSRect = msg_send![superview, bounds];
        let frame = inset(bounds, PILL_INSET);
        let _: () = msg_send![view, setFrame: frame];

        let radius = frame.size.height / 2.0;
        if responds(view, sel!(setCornerRadius:)) {
            let _: () = msg_send![view, setCornerRadius: radius];
        } else {
            let layer: *mut Object = msg_send![view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: radius];
            }
        }

        if let Ok(ns_window) = window.ns_window() {
            let ns_window = ns_window as *mut Object;
            if !ns_window.is_null() {
                let _: () = msg_send![ns_window, invalidateShadow];
            }
        }
    });
}

pub fn anchor(
    window: &tauri::WebviewWindow,
    anchor: PanelAnchor,
    margin: i32,
) -> Result<PanelStatus, String> {
    let ns_window = window.ns_window().map_err(|err| err.to_string())? as *mut Object;
    if ns_window.is_null() {
        return Err("window has no NSWindow yet".to_string());
    }

    unsafe {
        let _: () = msg_send![ns_window, setLevel: NS_STATUS_WINDOW_LEVEL];

        // Visible on every Space, skipped by Cmd-Tab and Mission Control
        // cycling, and allowed to sit over another app's fullscreen window.
        let behavior: u64 =
            CAN_JOIN_ALL_SPACES | TRANSIENT | IGNORES_CYCLE | FULL_SCREEN_AUXILIARY;
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];

        // Showing this window must not steal focus from whatever the user is
        // dictating into; without this the paste target changes mid-utterance.
        let mask: u64 = msg_send![ns_window, styleMask];
        let _: () = msg_send![ns_window, setStyleMask: mask | NONACTIVATING_PANEL];
        let _: () = msg_send![ns_window, setHidesOnDeactivate: NO];
        let _: () = msg_send![ns_window, setMovableByWindowBackground: YES];
    }

    // No layer-shell equivalent for edge anchoring, so place it geometrically.
    // Unlike the Linux fallback this is still a genuine panel: the level and
    // collection behaviour above are what matter, not how it got positioned.
    let status = super::fallback_position(window, anchor, margin)?;
    let _ = status;

    Ok(PanelStatus {
        anchored: true,
        layer_shell: false,
        detail: "Floating NSPanel at status-bar level, on all Spaces, never activating the app."
            .to_string(),
    })
}
