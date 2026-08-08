Target Platforms

- First-class: Linux + macOS.
- macOS: prioritize Apple Silicon with llama.cpp Accelerate support.
- Linux: ship CPU binary first, with optional app-managed NVIDIA CUDA/vLLM runtime in 0.0.2.
- Windows: not in v1.

Core Stack

- Desktop shell: Tauri v2.
- Frontend: Next.js static export + Tailwind CSS v4.
- Persistence: local SQLite in app data.
- ASR engine priority:
    1. Fun-ASR-Nano via GGUF/llama.cpp sidecar.
    2. Bundled Python worker fallback for functionality not available in GGUF yet.
    3. Optional GPU vLLM backend for Fun-ASR-Nano-2512 in 0.0.2, not default, installed into app data.

Tauri’s current Next.js guidance expects output: "export" and frontendDist: "../out", so the UI will avoid SSR/server actions. Tailwind
v4 will use the current @import "tailwindcss" setup with CSS-first theme tokens.

Runtime Design

- Tauri launches an ASR sidecar process.
- Main default sidecar: llama.cpp/Fun-ASR-Nano GGUF runtime.
- Python sidecar bundled for compatibility and download/model-management helpers.
- GPU sidecar: app-managed `uv` + Python 3.12 venv under app data. It installs FunASR, vLLM, PyTorch CUDA wheels, and CUDA shared-library wheels at runtime; the host still provides the NVIDIA driver/libcuda.
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
    - Runtime: GGUF CPU default, optional vLLM GPU, Python fallback.
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
- GPU runtime panel:
    - Detect NVIDIA driver/libcuda.
    - Install/repair app-managed GPU Python runtime.
    - Show Python, PyTorch CUDA, vLLM, FunASR, and CUDA device status.
- Available downloads:
    - Fun-ASR-Nano GGUF default.
    - Future: MLT Nano, SenseVoiceSmall, Paraformer.
    - Advanced: Fun-ASR-Nano-2512 vLLM GPU.

- Download/delete/retry controls.
- Default model/language/runtime.
- Hotwords where supported.
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

- Fun-ASR GGUF runtime API may not expose all Python features yet.
- True realtime partials need validation against a persistent streaming runtime; 0.0.2 uses bounded rolling-window preview to keep the UI responsive.
- Auto-paste needs per-platform implementation and permissions.
- Model artifact naming/download layout must be verified from Hugging Face before hardcoding.
- GPU vLLM runtime is large and depends on NVIDIA driver compatibility; default install uses CUDA 13.0 wheels and exposes `FUN_ASR_DESKTOP_TORCH_BACKEND` for older stacks. Default inference uses a conservative one-request, eager-mode vLLM profile for 8 GB GPUs.

Next Build Order

1. Scaffold Tauri + Next.js + Tailwind v4.
2. Add SQLite and settings/session persistence.
3. Add sidecar lifecycle and model manifest.
4. Integrate Fun-ASR-Nano GGUF download + smoke test.
5. Build main transcription UI.
6. Add realtime streaming partials.
7. Add floating voice bar with clipboard first, then auto-paste.
8. Package Linux/macOS dev builds.
