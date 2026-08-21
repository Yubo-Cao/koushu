//! Deliver text into whatever window is focused, and report how.
//!
//! The injection path cannot be verified by compiling it: every interesting
//! failure — a keymap that drops Han characters, a paste chord a terminal
//! ignores, a clipboard handoff that has not finished — happens at runtime in
//! another process. This binary makes that observable.
//!
//!     cargo run --example inject_probe -- "文本"
//!
//! It waits first, so the operator (or a script) can focus the target window.

use std::env;
use std::thread::sleep;
use std::time::Duration;

use koushu_lib::inject;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let text = if args.is_empty() {
        "ascii-ok 中文测试 mixed 混排".to_string()
    } else {
        args.join(" ")
    };
    let delay_ms: u64 = env::var("PROBE_DELAY_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000);
    let keep_clipboard = env::var("PROBE_KEEP_CLIPBOARD").is_ok();

    sleep(Duration::from_millis(delay_ms));

    let target = inject::capture_target();
    println!("target.app_id      = {:?}", target.app_id);
    println!("target.app_name    = {:?}", target.app_name);
    println!("target.pid         = {:?}", target.pid);
    println!("target.accepts_text= {:?}", target.accepts_text);
    println!("typeable           = {}", inject::is_typeable(&text));
    println!(
        "chord for target   = {}",
        inject::apps::chord_for(target.app_id.as_deref()).label()
    );

    let report = inject::inject(&text, &target, keep_clipboard);
    println!("delivered          = {}", report.delivered);
    println!("method             = {:?}", report.method);
    println!("chord sent         = {:?}", report.chord);
    println!("clipboard used     = {}", report.clipboard_used);
    println!("message            = {}", report.message);
}
