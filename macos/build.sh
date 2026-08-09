#!/bin/bash
# Build the prototype and assemble a real .app bundle.
#
# The bundle is not optional. A bare executable cannot carry
# NSMicrophoneUsageDescription, and without that key macOS does not refuse
# loudly — it hands the process an audio stream of pure silence, which is
# indistinguishable from a broken microphone.
#
# Signing: see sign-identity.sh. Short version — ad-hoc signing rotates the
# code identity on every build and silently voids the Accessibility and
# Microphone grants, so a stable self-signed identity is used instead. Set
# ADHOC=1 to opt back into ad-hoc and pay that cost.
set -euo pipefail
cd "$(dirname "$0")"

APP_NAME=FunASRBar
DEST=${DEST:-$HOME/Applications}
APP="$DEST/$APP_NAME.app"

KC="$HOME/.funasr-signing/funasr.keychain-db"
CN="FunASR Bar Dev"

swift build -c release --disable-sandbox
BIN=$(swift build -c release --show-bin-path)

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN/$APP_NAME" "$APP/Contents/MacOS/$APP_NAME"
cp Resources/Info.plist "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

if [ "${ADHOC:-0}" = "1" ]; then
    codesign --force --sign - "$APP"
else
    ./sign-identity.sh
    security unlock-keychain -p funasrproto "$KC"
    codesign --force --keychain "$KC" --sign "$CN" "$APP"
fi

echo "designated requirement:"
codesign -d -r- "$APP" 2>&1 | grep designated | sed 's/^/  /'
echo "built: $APP"
