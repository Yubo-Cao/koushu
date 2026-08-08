Target Platforms

- First-class: Linux + macOS.
- macOS: prioritize Apple Silicon with llama.cpp Accelerate support.
- Linux: CPU only. No GPU/CUDA runtime is shipped or planned (see README, "Why CPU only").
- Windows: not in v1.

Core Stack

- Desktop shell: Tauri v2.
- Frontend: Next.js static export + Tailwind CSS v4.
- Persistence: local SQLite in app data.
- ASR engines, both on the official Fun-ASR llama.cpp CPU runtime:
    1. Fun-ASR-Nano via `llama-funasr-cli` — default, ~8.8x realtime, best English.
    2. SenseVoiceSmall via `llama-funasr-sensevoice` — ~20.8x realtime, for speed.

Tauri’s current Next.js guidance expects output: "export" and frontendDist: "../out", so the UI will avoid SSR/server actions. Tailwind
v4 will use the current @import "tailwindcss" setup with CSS-first theme tokens.

Runtime Design

- Tauri launches an ASR sidecar process.
- Sidecars are the two official prebuilt binaries; model downloads are handled natively in Rust.
- Long audio is segmented by the runtime's built-in ggml FSMN-VAD, not an external front end.
- Tauri records microphone audio natively and sends bounded chunks to the local runtime.
- Runtime returns partial transcription events first, then final segments.
- Tauri handles native pieces: window lifecycle, app data paths, global shortcuts, clipboard, and paste automation.

Main Window

ChatGPT-style transcription workspace:

- Transcript timeline grouped by session.
- Live partial message while speaking.
- Final transcript card with copy, edit, retry, delete.
- Top controls:
    - Model: default Fun-ASR-Nano.
    - Language.
    - Model: Fun-ASR-Nano (accurate) or SenseVoiceSmall (fast).
    - Input device.

- Sessions saved by default.
- Sessions grouped by local date in the sidebar.

First Launch Setup

- Explain Fun-ASR and local/offline behavior.
- Detect platform, CPU, Apple Silicon, microphone access, available disk.
- Recommend Fun-ASR-Nano.
- Default download source: Hugging Face.
- Download GGUF model artifacts first.
- Run microphone smoke test.
- Run one short transcription test.
- Enter main app after a successful model check.

Configuration Window

- Installed models and disk usage.
- Runtime panel: engine, compute mode, platform.
- Available downloads:
    - Fun-ASR-Nano GGUF (default).
    - SenseVoiceSmall GGUF.
    - Future: MLT Nano, Paraformer.

- Download/delete/retry controls.
- Default model/language/runtime.
- VAD/punctuation/diarization controls only where backend supports them.
- Transcript retention settings.
- Audio retention off by default.
- Logs and diagnostics export.

Floating Voice Bar

- Separate frameless Tauri window.
- Always-on-top, draggable, compact.
- Mic button, live partial preview, final status.
- On final transcript:
    - Copy to clipboard.
    - Auto-paste into the focused app.

Important caveat: clipboard is straightforward via Tauri’s clipboard plugin; auto-paste is OS-sensitive. On macOS it will likely require
Accessibility permission. On Linux it depends on X11 vs Wayland, so I’ll implement a best-effort native paste layer with clear status/
fallback to clipboard when paste is blocked.

SQLite Schema

- sessions: id, title, started_at, ended_at, date_key, model, language, runtime.
- transcripts: id, session_id, text, partial/final status, created_at, duration_ms.
- segments: id, transcript_id, text, start_ms, end_ms, confidence nullable.
- settings: key/value.
- models: id, name, backend, source, local_path, status, size_bytes, installed_at.
- Add FTS5 index for transcript search.
- Enable WAL mode for normal desktop concurrency.

Main Open Risks

- The official CLI exposes no hotword or language flag; Nano detects language itself.
- True realtime partials still use a bounded rolling-window preview. At 8.8x realtime a 720 ms chunk costs ~82 ms, so genuine streaming is reachable on CPU without a persistent server.
- Auto-paste needs per-platform implementation and permissions.
- Model artifact naming/download layout must be verified from Hugging Face before hardcoding.

Next Build Order

1. Scaffold Tauri + Next.js + Tailwind v4.
2. Add SQLite and settings/session persistence.
3. Add sidecar lifecycle and model manifest.
4. Integrate Fun-ASR-Nano GGUF download + smoke test.
5. Build main transcription UI.
6. Add realtime streaming partials.
7. Add floating voice bar with clipboard first, then auto-paste.
8. Package Linux/macOS dev builds.
