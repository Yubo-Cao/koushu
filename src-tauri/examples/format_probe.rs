//! End-to-end check of the formatting layer against the real database:
//! read a transcript, stream a format pass, persist it, read it back.
//! Usage: cargo run --example format_probe -- <transcript_id>
use rusqlite::{params, Connection};

fn main() {
    let id = std::env::args().nth(1).expect("transcript id");
    let db = dirs_next::data_dir()
        .unwrap()
        .join("dev.yubo.fun-asr-desktop/fun_asr_desktop.sqlite3");
    let conn = Connection::open(&db).expect("open db");

    let get = |key: &str| -> String {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };
    let config = fun_asr_desktop_lib::llm::LlmConfig {
        base_url: get("llm.baseUrl"),
        model: get("llm.model"),
        api_key: String::new(),
        temperature: None,
    };
    let preset_id = {
        let v = get("llm.preset");
        if v.is_empty() { "typeset".to_string() } else { v }
    };
    let preset = fun_asr_desktop_lib::llm::presets::by_id(&preset_id).expect("preset");

    let text: String = conn
        .query_row("SELECT text FROM transcripts WHERE id = ?1", params![id], |r| r.get(0))
        .expect("transcript");
    println!("raw       : {text}");
    println!("endpoint  : {} model={}", config.base_url, config.model);

    let mut deltas = 0;
    let formatted = fun_asr_desktop_lib::llm::format_streaming(
        &config, preset.prompt, &text, |_| deltas += 1, || false,
    )
    .expect("format");
    println!("formatted : {formatted}");
    println!("deltas    : {deltas}");

    conn.execute(
        "UPDATE transcripts SET formatted_text=?1, formatted_preset=?2, formatted_at=datetime('now') WHERE id=?3",
        params![formatted, preset_id, id],
    )
    .expect("persist");

    let (stored, stored_preset, at): (String, String, String) = conn
        .query_row(
            "SELECT formatted_text, formatted_preset, formatted_at FROM transcripts WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("read back");
    println!("\n[persisted] preset={stored_preset} at={at}");
    println!("[persisted] {stored}");
    println!("[raw intact] {}",
        conn.query_row("SELECT text FROM transcripts WHERE id=?1", params![id],
            |r| r.get::<_, String>(0)).unwrap() == text);
}
