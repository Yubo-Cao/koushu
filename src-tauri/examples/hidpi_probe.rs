//! What GDK actually reports on a mixed-DPI Wayland desk.
//!
//! The panel geometry code converts between logical and physical pixels using
//! `monitor.scale_factor()`. On Linux that number comes from GDK, which only
//! ever knows *integer* scales — so on an output the compositor runs at 1.25
//! the conversion is against a value the compositor never used. This prints
//! both sides so the mismatch is a measurement rather than a guess.
//!
//! Run under the Plasma session bus:
//!   cargo run --example hidpi_probe

#[cfg(target_os = "linux")]
fn main() {
    use gtk::prelude::*;

    if gtk::init().is_err() {
        eprintln!("gtk::init failed — is WAYLAND_DISPLAY/DISPLAY set?");
        std::process::exit(1);
    }

    let display = gtk::gdk::Display::default().expect("no default display");
    println!("backend           : {}", display.type_().name());
    println!("n_monitors        : {}", display.n_monitors());
    println!();

    for i in 0..display.n_monitors() {
        let Some(monitor) = display.monitor(i) else {
            continue;
        };
        let geom = monitor.geometry();
        let scale = monitor.scale_factor();
        println!("monitor[{i}] {:?}", monitor.model());
        println!(
            "  gdk geometry (LOGICAL) : {},{} {}x{}",
            geom.x(),
            geom.y(),
            geom.width(),
            geom.height()
        );
        println!("  gdk scale_factor       : {scale}  (integer only)");
        println!(
            "  tao monitor.size()     : {}x{}   tao monitor.position(): {},{}",
            geom.width() * scale,
            geom.height() * scale,
            geom.x() * scale,
            geom.y() * scale,
        );
        println!();
    }

    // A realized window reports the scale of whichever monitor GDK thinks it
    // is on. Pump the loop properly: the scale only settles once the
    // compositor has sent wl_surface.enter, which is several roundtrips after
    // show().
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(190, 44);
    window.set_title("hidpi-probe");
    window.show_all();

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2500);
    let mut last = String::new();
    while std::time::Instant::now() < deadline {
        gtk::main_iteration_do(false);
        std::thread::sleep(std::time::Duration::from_millis(20));

        let Some(gdk_window) = window.window() else {
            continue;
        };
        let win_scale = window.scale_factor();
        let mon = display.monitor_at_window(&gdk_window);
        let mon_name = mon.as_ref().and_then(|m| m.model()).map(|s| s.to_string());
        let mon_scale = mon.as_ref().map(|m| m.scale_factor()).unwrap_or(-1);
        let (gw, gh) = window.size();
        let origin = gdk_window.origin();
        let line = format!(
            "window: gtk_size(LOGICAL)={gw}x{gh} window.scale_factor={win_scale} \
             on={mon_name:?} monitor.scale={mon_scale} gdk_origin={},{}",
            origin.1, origin.2
        );
        if line != last {
            println!("[{:>5}ms] {line}", 2500 - (deadline - std::time::Instant::now()).as_millis());
            last = line;
        }
    }
    println!();
    println!("FINAL: {last}");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("linux only");
}
