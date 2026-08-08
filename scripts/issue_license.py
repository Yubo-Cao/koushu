#!/usr/bin/env python3
"""Generate the signing keypair and issue offline licences.

    ./issue_license.py keygen                      # once; prints the public key
    ./issue_license.py sign buyer@example.com      # per purchase

The private key stays on your machine. Put the printed public key into
PUBLIC_KEY_HEX in src-tauri/src/license.rs — the client verifies against it
with no network access.

Requires: pip install cryptography
"""
import base64
import json
import os
import sys
from datetime import date

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)
from cryptography.hazmat.primitives import serialization

KEY_PATH = os.path.expanduser("~/.config/fun-asr-desktop/license-signing.key")


def b64(data: bytes) -> str:
    """URL-safe, unpadded — keeps a licence to one line that survives email."""
    return base64.urlsafe_b64encode(data).decode().rstrip("=")


def keygen() -> None:
    if os.path.exists(KEY_PATH):
        sys.exit(f"refusing to overwrite an existing key at {KEY_PATH}")
    os.makedirs(os.path.dirname(KEY_PATH), exist_ok=True)
    private = Ed25519PrivateKey.generate()
    raw = private.private_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PrivateFormat.Raw,
        encryption_algorithm=serialization.NoEncryption(),
    )
    # 0600: this file is the only thing standing between you and anyone
    # issuing their own licences.
    fd = os.open(KEY_PATH, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as handle:
        handle.write(raw)

    public = private.public_key().public_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PublicFormat.Raw,
    )
    print(f"private key written to {KEY_PATH} (mode 0600) — back this up offline")
    print("\nput this into PUBLIC_KEY_HEX in src-tauri/src/license.rs:\n")
    print(f'const PUBLIC_KEY_HEX: &str = "{public.hex()}";')


def sign(email: str, version: str | None) -> None:
    if not os.path.exists(KEY_PATH):
        sys.exit("no signing key yet — run: issue_license.py keygen")
    with open(KEY_PATH, "rb") as handle:
        private = Ed25519PrivateKey.from_private_bytes(handle.read())

    payload = {"email": email, "issued": date.today().isoformat()}
    if version:
        payload["version"] = version
    # separators matter: the client verifies the exact bytes that were signed,
    # so the encoding has to be stable.
    raw = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode()
    signature = private.sign(raw)
    print(f"FUNASR-{b64(raw)}.{b64(signature)}")


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    if sys.argv[1] == "keygen":
        keygen()
    elif sys.argv[1] == "sign":
        if len(sys.argv) < 3:
            sys.exit("usage: issue_license.py sign <email> [version]")
        sign(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)
    else:
        sys.exit(__doc__)


main()
