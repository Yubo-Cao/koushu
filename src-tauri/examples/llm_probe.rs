//! Exercise the streaming LLM client against any OpenAI-compatible endpoint.
//! Usage: cargo run --example llm_probe -- <base_url> <model> [api_key]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let config = fun_asr_desktop_lib::llm::LlmConfig {
        base_url: a.get(1).cloned().unwrap_or_default(),
        model: a.get(2).cloned().unwrap_or_else(|| "test-model".into()),
        api_key: a.get(3).cloned().unwrap_or_default(),
        temperature: None,
    };
    let mut deltas = 0usize;
    let result = fun_asr_desktop_lib::llm::format_streaming(
        &config,
        fun_asr_desktop_lib::llm::presets::TYPESET.prompt,
        "嗯 那个 我们今天 我们今天讨论一下这个 push to talk 的实现",
        |d| {
            deltas += 1;
            print!("{d}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        },
        || false,
    );
    println!();
    match result {
        Ok(text) => println!("\n[ok] {deltas} deltas, {} chars", text.chars().count()),
        Err(err) => println!("\n[err] {err}"),
    }
}
