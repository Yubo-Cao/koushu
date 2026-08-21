#!/bin/bash
# Build Koushu and assemble a real .app bundle.
#
# The bundle is not optional. A bare executable cannot carry
# NSMicrophoneUsageDescription, and without that key macOS does not refuse
# loudly — it hands the process an audio stream of pure silence, which is
# indistinguishable from a broken microphone.
#
# Signing: see sign-identity.sh. Short version — ad-hoc signing rotates the code
# identity on every build and silently voids the Accessibility and Microphone
# grants, so a stable self-signed identity is used instead. Set ADHOC=1 to opt
# back into ad-hoc and pay that cost.
#
# The shared Rust core is optional here, and that is deliberate: the generated
# Swift bindings are gitignored, so a fresh clone has none, and a build that
# required them would not work at all. Pass CORE=1 to regenerate and link them.
# Without it the app builds against the Swift protocol stubs in KoushuCore —
# every window works, and transcription says in its own text that it is a
# placeholder.
set -euo pipefail
cd "$(dirname "$0")"

APP_NAME=Koushu
BIN_NAME=Koushu
DEST=${DEST:-$HOME/Applications}
APP="$DEST/$APP_NAME.app"
REPO_ROOT="$(cd .. && pwd)"

KC="$HOME/.koushu-signing/koushu.keychain-db"
CN="Koushu Dev"

# ---------------------------------------------------------------- Rust core --
#
# SPM refuses target paths outside the package directory, and the generated
# bindings live in ../core/generated. So they are staged into two gitignored
# target directories here, and Package.swift decides whether to include the
# targets by looking for the staged files. That check is the whole mechanism:
# there is no flag to keep in step, and a stale staging directory cannot make
# the build silently use old bindings, because this script always rewrites it.
GEN="$REPO_ROOT/core/generated/swift"

stage_core() {
    echo "==> regenerating Swift bindings for koushu-core"
    # Match the app's LSMinimumSystemVersion. Without it, cargo builds the C
    # parts of `ring` for whatever the host runs, and the linker warns — once per
    # object file — that they were built for a newer macOS than the thing linking
    # them. Which is also a real statement about what the .a would do on a
    # machine older than this one.
    MACOSX_DEPLOYMENT_TARGET=26.0 "$REPO_ROOT/scripts/gen-swift-bindings.sh"

    mkdir -p Sources/KoushuCoreFFI/include
    cp "$GEN/koushu_core.swift" Sources/KoushuRustCore/koushu_core.swift
    cp "$GEN/koushu_coreFFI.h" Sources/KoushuCoreFFI/include/koushu_coreFFI.h

    # The static library is copied in rather than linked where cargo left it.
    # This machine sets a global `target-dir` in .cargo/config.toml, so "the
    # repository's target directory" is not a path that exists; asking cargo is
    # the only reliable answer, and copying the answer here means Package.swift
    # needs no knowledge of cargo at all.
    local target_dir
    target_dir="$(cd "$REPO_ROOT" && cargo metadata --format-version 1 --no-deps \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
    mkdir -p .rustlib
    cp "$target_dir/release/libkoushu_core.a" .rustlib/libkoushu_core.a

    # The generated modulemap assumes the header sits beside it; a systemLibrary
    # target wants it under include/. Written here rather than copied, so the
    # two cannot disagree about the path.
    cat > Sources/KoushuCoreFFI/module.modulemap <<'EOF'
module koushu_coreFFI {
    header "include/koushu_coreFFI.h"
    export *
}
EOF
    echo "==> staged bindings into Sources/{KoushuRustCore,KoushuCoreFFI}"
}

unstage_core() {
    rm -f Sources/KoushuRustCore/koushu_core.swift
    rm -rf Sources/KoushuCoreFFI .rustlib
}

if [ "${CORE:-0}" = "1" ]; then
    stage_core
else
    # Not an error, and not silent: a build without the core is a build whose
    # transcription is a placeholder, and that has to be visible in the log
    # rather than discovered from a screenshot.
    unstage_core
    echo "==> building WITHOUT the Rust core (stubs only). Pass CORE=1 to link it."
fi

# -------------------------------------------------------------------- build --
swift build -c release --disable-sandbox
BIN=$(swift build -c release --show-bin-path)

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN/$BIN_NAME" "$APP/Contents/MacOS/$BIN_NAME"
cp Resources/Info.plist "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# Generate the native icon from the shared 1024 px master so Linux and macOS
# ship the same Koushu identity without checking in derived icon formats.
ICONSET="$(mktemp -d)/Koushu.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" ../src-tauri/icons/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    size2=$((size * 2))
    sips -z "$size2" "$size2" ../src-tauri/icons/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Koushu.icns"

# The official Fun-ASR llama.cpp runtimes, in the same place the Tauri bundle
# puts them, so the two builds can be compared without also arguing about
# layout. Only this architecture's copies: the repository holds every
# platform's, and shipping the Linux ones inside a Mac app would add 11 MB of
# files that can never run.
mkdir -p "$APP/Contents/Resources/binaries"
cp ../src-tauri/binaries/*-aarch64-apple-darwin "$APP/Contents/Resources/binaries/"
chmod +x "$APP/Contents/Resources/binaries/"*

# The prototype's bundle, if it is still there. Two menu-bar icons for the same
# app is worse than either one alone.
rm -rf "$DEST/FunASRBar.app"

# Nested executables are signed first, then the bundle.
#
# Not `--deep`, which Apple deprecated and which signs things in an order that
# is not always the right one. Inside-out is the documented order, and it
# matters here: an unsigned binary inside a signed bundle makes the bundle's
# seal invalid, and macOS refuses to launch the *app* over a runtime it was
# never going to check anyway.
sign() {
    if [ "${ADHOC:-0}" = "1" ]; then
        codesign --force --sign - "$1"
    else
        codesign --force --keychain "$KC" --sign "$CN" "$1"
    fi
}

if [ "${ADHOC:-0}" != "1" ]; then
    ./sign-identity.sh
    security unlock-keychain -p koushudev "$KC"
fi

for binary in "$APP/Contents/Resources/binaries/"*; do
    sign "$binary"
done
sign "$APP"

echo "designated requirement:"
codesign -d -r- "$APP" 2>&1 | grep designated | sed 's/^/  /'
echo "built: $APP"
