#!/usr/bin/env bash
# Regenerate the Swift bindings for koushu-core.
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
cargo build -p koushu-core --release

# Ask cargo where it put things rather than assuming `./target`.
#
# Two things move it and neither is visible from here: a workspace puts the
# output at the workspace root instead of beside the crate, and a global
# `target-dir` in .cargo/config.toml — which this project's host machine sets,
# to share one build directory across every checkout — moves it off the tree
# entirely. Hard-coding the path failed on the second, with an error that reads
# as "the build did not happen" when the build had in fact succeeded.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

LIB="${TARGET_DIR}/release/libkoushu_core.dylib"
[ -f "${LIB}" ] || LIB="${TARGET_DIR}/release/libkoushu_core.so"
if [ ! -f "${LIB}" ]; then
  echo "no cdylib at ${TARGET_DIR}/release/libkoushu_core.{dylib,so}" >&2
  exit 1
fi

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"
# `--features bindgen-cli` is required, not optional: the binary is declared
# with `required-features` so that an ordinary build of the library does not
# drag uniffi's CLI in. Without the flag cargo refuses to build the target at
# all, and the message names the feature rather than the fix.
cargo run -p koushu-core --features bindgen-cli --bin uniffi-bindgen -- \
  generate --library "${LIB}" --language swift --out-dir "${OUT_DIR}"

echo "Swift bindings written to ${OUT_DIR}"
