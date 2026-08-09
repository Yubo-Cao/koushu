#!/usr/bin/env bash
# Regenerate the Swift bindings for fun-asr-core.
#
# The output is gitignored on purpose. Checked-in generated code drifts from
# the source it was generated from and nobody notices until a signature changes
# under them; regenerating is cheap and always tells the truth.
#
# Run this after changing anything marked `#[uniffi::export]`, and from the
# macOS build before compiling Swift.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/core/generated/swift"

cd "${ROOT_DIR}"

# Build the cdylib first: uniffi-bindgen reads the compiled library rather than
# the source, which is what keeps the bindings honest about what was exported.
cargo build -p fun-asr-core --release

LIB="${ROOT_DIR}/target/release/libfun_asr_core.dylib"
[ -f "${LIB}" ] || LIB="${ROOT_DIR}/target/release/libfun_asr_core.so"
if [ ! -f "${LIB}" ]; then
  echo "no cdylib at target/release/libfun_asr_core.{dylib,so}" >&2
  exit 1
fi

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"
cargo run -p fun-asr-core --bin uniffi-bindgen -- \
  generate --library "${LIB}" --language swift --out-dir "${OUT_DIR}"

echo "Swift bindings written to ${OUT_DIR}"
