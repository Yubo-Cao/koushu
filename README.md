# Koushu

Linux-first Tauri wrapper for Fun-ASR with a Next.js + Tailwind CSS v4 frontend.

## 0.0.2 Scope

- Tauri v2 desktop shell with main, settings, and floating voice-bar windows.
- Bun-managed Next.js static export frontend.
- Local SQLite persistence for settings, models, sessions, transcripts, segments, and FTS.
- Bundled official Fun-ASR llama.cpp CPU runtime (`llama-funasr-cli` + `llama-funasr-sensevoice`, release `runtime-llamacpp-v0.1.9`), with Fun-ASR-Nano and SenseVoiceSmall selectable per transcription.
- Native Linux microphone capture through Tauri/Rust, with bounded rolling-window partial previews to avoid UI stalls.
- Hugging Face model management for the official `FunAudioLLM/*-GGUF` weights.
- CPU only: no GPU, no CUDA, and no Python at build or run time.
- Linux clipboard and auto-paste path modeled after espanso: snapshot, set text, paste shortcut, conditional restore.

## Develop

```bash
bun install
bun run tauri:dev
```

The frontend build uses Bun's runtime for Next.js:

```bash
bun run build
```

## Build Linux

Local host build for `.deb`:

```bash
bun run tauri:build
```

The verified default artifact is:

```text
target/release/bundle/deb/Koushu_0.0.2_amd64.deb
```

The release binary is also available at:

```text
target/release/koushu
```

Local host AppImage build:

```bash
bun run tauri:build:appimage
```

The verified AppImage artifact is:

```text
target/release/bundle/appimage/Koushu-0.0.2-x86_64.AppImage
```

The host AppImage script keeps Tauri's normal AppDir generation, then retries only
the AppImage packaging step if host `linuxdeploy` trips over rolling-distro GTK
library scans or old `strip` handling for `.relr.dyn` sections.

For reproducible `.deb` plus AppImage builds, use the devcontainer/Docker builder:

```bash
bun run tauri:build:linux-container
```

This uses `.devcontainer/Dockerfile` to build inside Ubuntu 24.04 with the Tauri Linux dependencies installed, avoiding host-library contamination from rolling Linux workstations.

On this machine, Docker apt mirrors were unreliable during testing; the host
AppImage build above is the verified release path.

## First Launch

The welcome page downloads the GGUF weights from Hugging Face into app data. The
ASR runtime itself ships inside the installer, so there is nothing else to set up:
no Python, no CUDA, no GPU.

| Model | Files | Size | Speed (CPU) |
|---|---|---|---|
| Fun-ASR-Nano (default) | `funasr-encoder-f16.gguf`, `qwen3-0.6b-q4km.gguf`, `fsmn-vad.gguf` | ~955 MB | ~8.8x realtime |
| SenseVoiceSmall | `sensevoice-small-q8.gguf`, `fsmn-vad.gguf` | ~256 MB | ~20.8x realtime |

Measured on a 24-core Intel Core Ultra 9 275HX with a 30.5 s clip. Both models
segment long audio with the runtime's built-in ggml FSMN-VAD, and both emit
punctuation and inverse text normalisation on their own.

Fun-ASR-Nano is the default because it is markedly stronger on English. On the
same clip, SenseVoice rendered "we don't need to rely on" as "we rely on" —
a reversal of meaning — while Nano transcribed it correctly. SenseVoice is the
right pick when speed matters more, for example on long recordings.

## Why CPU only

There is no GPU path, deliberately. Upstream ships no Linux CUDA build (only
Windows), and its Linux Vulkan package contains only the SenseVoice binary,
which fails silently on NVIDIA and Intel alike. The vLLM route needs roughly
3.4 GB of VRAM on top of a 1.04 GB encoder, so on an 8 GB laptop GPU already
running a desktop it OOMs while loading the embedding layer.

None of that costs accuracy: CPU and GPU run identical weights. Quantisation to
Q8 moves CER by about 0.3% against the fp32 reference. At 8.8x realtime, a
720 ms audio chunk takes roughly 82 ms to process, which is well inside what
near-realtime transcription needs.

## Linux Paste Dependencies

Clipboard copy uses the native Rust clipboard path first, then falls back to:

- Wayland: `wl-copy`, `wl-paste`
- X11: `xclip` or `xsel`

Auto-paste sends a paste shortcut after copying:

- Wayland: `Shift+Insert` via `wtype` or `ydotool`
- X11: `Ctrl+V` via `xdotool` or `xte`

If paste injection is unavailable, the app keeps the transcript copied to the clipboard and reports the missing tool.
