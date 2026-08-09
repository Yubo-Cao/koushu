#!/bin/bash
# Create a stable local code-signing identity, once.
#
# Why this exists
# ---------------
# Ad-hoc signing (`codesign -s -`) makes the *designated requirement* be the
# code hash itself:
#
#     designated => cdhash H"aecb8af5..."
#
# TCC stores that requirement alongside a grant, so every rebuild produces a
# program macOS has never seen before and both the Accessibility and Microphone
# grants silently stop applying. The checkbox in System Settings stays ticked and
# does nothing, which is the worst possible failure mode. The cure is the −/+
# dance in System Settings, per build.
#
# Signing with a self-signed certificate instead makes the requirement:
#
#     designated => identifier "com.funasr.voicebar.prototype"
#                   and certificate leaf = H"d660d124..."
#
# The certificate does not change when the code changes, so a grant given once
# survives every subsequent build.
#
# Scope of what this touches: one keychain under ~/.funasr-signing, holding one
# certificate. It does **not** add a trusted root to the system trust store and
# does not modify the keychain search list — `codesign` is pointed at the
# keychain explicitly instead. Undo with:  rm -rf ~/.funasr-signing
set -euo pipefail

WORK="$HOME/.funasr-signing"
KC="$WORK/funasr.keychain-db"
PW="funasrproto"
CN="FunASR Bar Dev"

if [ -f "$KC" ] && security find-certificate -c "$CN" "$KC" >/dev/null 2>&1; then
    security unlock-keychain -p "$PW" "$KC"
    echo "signing identity already present: $CN"
    exit 0
fi

mkdir -p "$WORK"
cd "$WORK"

cat > ext.cnf <<'EOF'
[req]
distinguished_name=dn
x509_extensions=v3
prompt=no
[dn]
CN=FunASR Bar Dev
[v3]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
EOF

openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem \
    -days 3650 -nodes -config ext.cnf 2>/dev/null

# Security.framework cannot read OpenSSL 3's default PKCS#12 encryption; the
# import fails with "MAC verification failed (wrong password?)", which is a lie.
openssl pkcs12 -export -out id.p12 -inkey key.pem -in cert.pem \
    -passout "pass:$PW" -name "$CN" \
    -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1 2>/dev/null

security create-keychain -p "$PW" "$KC"
security unlock-keychain -p "$PW" "$KC"
security set-keychain-settings -lut 100000 "$KC"   # don't auto-lock mid-build
security import id.p12 -k "$KC" -P "$PW" -A
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$PW" "$KC" >/dev/null 2>&1 || true

rm -f id.p12 key.pem
echo "created signing identity: $CN"
