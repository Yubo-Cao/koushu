//! Exercise the cloud ASR client against any OpenAI-compatible endpoint.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cfg = koushu_lib::asr_cloud::CloudAsrConfig {
        base_url: a.get(1).cloned().unwrap_or_default(),
        model: a.get(2).cloned().unwrap_or_else(|| "whisper-1".into()),
        api_key: a.get(3).cloned().unwrap_or_default(),
        language: String::new(),
    };
    let wav = a.get(4).cloned().unwrap_or_default();
    match koushu_lib::asr_cloud::transcribe(&cfg, std::path::Path::new(&wav)) {
        Ok(t) => println!("[ok] {t}"),
        Err(e) => println!("[err] {e}"),
    }
}
