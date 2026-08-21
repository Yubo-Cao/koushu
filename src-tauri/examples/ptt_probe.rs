//! Manual probe: start the push-to-talk listener and print every edge.
//! Run with `cargo run --example ptt_probe -- "CTRL+ALT+space" 12`
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let trigger = args.get(1).cloned().unwrap_or_else(|| "CTRL+ALT+space".into());
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12);

    let start = Instant::now();
    let status = koushu_lib::hotkey_start_for_probe(&trigger, move |edge| {
        println!("  {:?}  @{:.2}s", edge, start.elapsed().as_secs_f32());
    });
    println!("backend = {:?}", status.backend);
    println!("trigger = {}", status.trigger);
    // The one to read. A binding can be accepted and still not be the chord
    // that was asked for.
    println!("ok      = {}", status.ok);
    println!("bound   = {:?}", status.bound_description);
    println!("detail  = {}", status.detail);
    println!("listening {secs}s ...");
    std::thread::sleep(Duration::from_secs(secs));
    println!("done");
}
