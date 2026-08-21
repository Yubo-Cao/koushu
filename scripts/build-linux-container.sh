#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_NAME="${FUN_ASR_DESKTOP_BUILDER_IMAGE:-koushu-linux-builder:0.0.1}"

docker build \
  -f "${ROOT_DIR}/.devcontainer/Dockerfile" \
  --build-arg "UBUNTU_MIRROR=${UBUNTU_MIRROR:-http://mirrors.edge.kernel.org/ubuntu/}" \
  -t "${IMAGE_NAME}" \
  "${ROOT_DIR}/.devcontainer"

docker run --rm \
  --user "$(id -u):$(id -g)" \
  -e HOME=/tmp/fun-asr-home \
  -e CARGO_HOME=/workspace/.cargo-container \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e NO_STRIP=true \
  -e WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  -v "${ROOT_DIR}:/workspace" \
  -w /workspace \
  "${IMAGE_NAME}" \
  bash -lc '
    set -euo pipefail
    mkdir -p "$HOME" "$CARGO_HOME"
    export PATH="/usr/local/cargo/bin:/usr/local/bun/bin:$PATH"
    bun install --frozen-lockfile
    bunx tauri build --bundles deb appimage
  '
