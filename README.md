# Fun ASR Desktop

Linux-first Tauri wrapper for Fun-ASR with a Next.js + Tailwind CSS v4 frontend.

## 0.0.2 Scope

- Tauri v2 desktop shell with main, settings, and floating voice-bar windows.
- Bun-managed Next.js static export frontend.
- Local SQLite persistence for settings, models, sessions, transcripts, segments, and FTS.
- Bundled official Fun-ASR llama.cpp CPU runtime (`llama-funasr-cli` + `llama-funasr-sensevoice`, release `runtime-llamacpp-v0.1.9`), with Fun-ASR-Nano and SenseVoiceSmall selectable per transcription.
- Native Linux microphone capture through Tauri/Rust, with bounded rolling-window partial previews to avoid UI stalls.
- Hugging Face model management for `cstr/funasr-nano-GGUF` Q4_K.
- Optional GPU vLLM inference path for `FunAudioLLM/Fun-ASR-Nano-2512` through the Python bridge.
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
src-tauri/target/release/bundle/deb/Fun ASR Desktop_0.0.2_amd64.deb
```

The release binary is also available at:

```text
src-tauri/target/release/fun-asr-desktop
```

Local host AppImage build:

```bash
bun run tauri:build:appimage
```

The verified AppImage artifact is:

```text
src-tauri/target/release/bundle/appimage/Fun_ASR_Desktop-0.0.2-x86_64.AppImage
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

The welcome page downloads `funasr-nano-2512-q4_k.gguf` from Hugging Face into app data. The ASR runtime itself is bundled in the installer, so users do not need Python for the default path.

## Optional GPU vLLM Backend

The app shell and default GGUF runtime do not need Python or CUDA. The 0.0.2 vLLM backend is optional and intended for NVIDIA GPU workstations with a host NVIDIA driver.

Open Settings, use **Install GPU Runtime**, then download/select `Fun-ASR-Nano vLLM GPU`. The AppImage keeps the GPU stack in app data:

- `runtimes/uv`: a downloaded `uv` bootstrap binary if `uv` is not already on PATH.
- `runtimes/vllm-gpu/venv`: managed Python 3.12.
- `runtimes/uv-cache`: PyTorch CUDA, vLLM, FunASR, and NVIDIA CUDA wheel caches.

The default backend is `cu130`, which installs CUDA 13.0 PyTorch wheels and downloads the needed CUDA shared libraries at runtime. The host still must provide the NVIDIA driver and `libcuda.so`. Override the backend for older driver stacks with:

```bash
FUN_ASR_DESKTOP_TORCH_BACKEND=cu128 ./Fun_ASR_Desktop-0.0.2-x86_64.AppImage
```

The default vLLM profile is intentionally conservative for 8 GB laptop GPUs: `gpu_memory_utilization=0.50`, `max_model_len=2048`, `max_num_seqs=1`, and eager mode enabled. Override only when you have more free VRAM:

```bash
FUN_ASR_DESKTOP_VLLM_GPU_MEMORY=0.75 \
FUN_ASR_DESKTOP_VLLM_MAX_NUM_SEQS=4 \
FUN_ASR_DESKTOP_VLLM_ENFORCE_EAGER=false \
./Fun_ASR_Desktop-0.0.2-x86_64.AppImage
```

This path uses `FunAudioLLM/Fun-ASR-Nano-2512` from Hugging Face and calls the bridge's `transcribe-vllm` command through the managed Python runtime.

## Optional Python CPU Fallback

The Python CPU bridge is kept for fallback experiments and needs:

```bash
python3 -m pip install -r src-tauri/python/requirements.txt
```

You can override the Python executable:

```bash
FUN_ASR_DESKTOP_PYTHON=/path/to/python bun run tauri:dev
```

## Linux Paste Dependencies

Clipboard copy uses the native Rust clipboard path first, then falls back to:

- Wayland: `wl-copy`, `wl-paste`
- X11: `xclip` or `xsel`

Auto-paste sends a paste shortcut after copying:

- Wayland: `Shift+Insert` via `wtype` or `ydotool`
- X11: `Ctrl+V` via `xdotool` or `xte`

If paste injection is unavailable, the app keeps the transcript copied to the clipboard and reports the missing tool.
