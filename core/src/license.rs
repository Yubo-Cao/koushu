//! Offline licence verification.
//!
//! A licence is a signed statement, not a lookup key. It is checked entirely
//! on-device against a public key compiled into the binary, so activation
//! works on a plane and nothing about the user's usage is ever reported.
//!
//! **Why not an online check.** It would make the paid build strictly worse
//! than the free one: it could fail while offline, which is the exact
//! situation this app is built for. It would also be pointless as protection —
//! anyone able to patch out a signature check can already build from source,
//! and that person was never a customer. The check exists to make paying
//! meaningful for people who want to pay, not to stop people who don't.
//!
//! Format: `FUNASR-<base64url(payload)>.<base64url(signature)>`
//! where payload is the canonical JSON below. It is one line, so it survives
//! being pasted through email and chat clients intact.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Ed25519 public key, hex-encoded. The matching private key signs licences
/// and never leaves the seller's machine.
///
/// The placeholder below verifies nothing; replace it at release time. Left
/// obviously invalid on purpose so a build that forgot to set it fails closed
/// rather than accepting every licence.
const PUBLIC_KEY_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

const PREFIX: &str = "FUNASR-";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    /// Who it was issued to. Shown in the UI so a licence is identifiable.
    pub email: String,
    /// Issue date, ISO 8601. Purely informational.
    pub issued: String,
    /// Major version this covers, so per-version pricing stays possible
    /// without reissuing infrastructure.
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInfo {
    pub valid: bool,
    pub email: Option<String>,
    pub issued: Option<String>,
    /// Why a licence was rejected, in words the user can act on.
    pub detail: String,
}

// A rejected licence deliberately does **not** cross the boundary as an error.
//
// `verify` never returns a `Result`, because "this licence is not valid" is an
// answer, not a failure — and the answer carries text the user can act on. An
// earlier version re-modelled that as a UniFFI error, which cost more than it
// looked: UniFFI generates `errorDescription` as `String(reflecting: self)`, so
// the idiomatic Swift call — putting `error.localizedDescription` in front of
// the user — would have shown
// `fun_asr_core.LicenseVerificationError.Rejected(detail: "…")` rather than the
// sentence inside it. It also made `LicenseInfo.valid` a field that could only
// ever be `true` on the Swift side.
//
// So the export below returns `LicenseInfo` directly. Callers read `valid` and
// show `detail`, on every platform, with no translation layer to keep in sync.

fn verifying_key() -> Option<VerifyingKey> {
    let bytes = (0..PUBLIC_KEY_HEX.len() / 2)
        .map(|i| u8::from_str_radix(&PUBLIC_KEY_HEX[i * 2..i * 2 + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    let bytes: [u8; 32] = bytes.try_into().ok()?;
    // An all-zero key is the unset placeholder, never a real key.
    if bytes.iter().all(|b| *b == 0) {
        return None;
    }
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Verify a licence string. Never panics on malformed input — every failure
/// path returns a description rather than an error the caller has to render.
pub fn verify(license: &str) -> LicenseInfo {
    let reject = |detail: &str| LicenseInfo {
        valid: false,
        email: None,
        issued: None,
        detail: detail.to_string(),
    };

    let trimmed = license.trim();
    let Some(body) = trimmed.strip_prefix(PREFIX) else {
        return reject("That does not look like a licence key.");
    };
    let Some((payload_b64, signature_b64)) = body.split_once('.') else {
        return reject("The licence key is incomplete — it should contain a '.'");
    };

    let Ok(payload_bytes) = URL_SAFE_NO_PAD.decode(payload_b64) else {
        return reject("The licence key is damaged; check it was copied whole.");
    };
    let Ok(signature_bytes) = URL_SAFE_NO_PAD.decode(signature_b64) else {
        return reject("The licence key is damaged; check it was copied whole.");
    };
    let Ok(signature_bytes) = <[u8; 64]>::try_from(signature_bytes.as_slice()) else {
        return reject("The licence signature has the wrong length.");
    };

    let Some(key) = verifying_key() else {
        // Fail closed: a build without a real key must not accept anything.
        return reject("This build has no licence key configured.");
    };

    // Verify before parsing: never let unverified bytes reach the parser.
    let signature = Signature::from_bytes(&signature_bytes);
    if key.verify_strict(&payload_bytes, &signature).is_err() {
        return reject("This licence is not valid for this application.");
    }

    match serde_json::from_slice::<LicensePayload>(&payload_bytes) {
        Ok(payload) => LicenseInfo {
            valid: true,
            email: Some(payload.email),
            issued: Some(payload.issued),
            detail: "Licence verified.".to_string(),
        },
        Err(_) => reject("The licence contents could not be read."),
    }
}

/// Verify a licence from a native caller.
///
/// Thin on purpose: it is [`verify`] with an owned `String`, which is what the
/// FFI can carry. Every decision stays in one place, so the Swift and Tauri
/// shells cannot drift apart on what a rejected licence means.
#[uniffi::export]
pub fn verify_license(license: String) -> LicenseInfo {
    verify(&license)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_without_panicking() {
        for input in [
            "",
            "hello",
            "FUNASR-",
            "FUNASR-nodot",
            "FUNASR-!!!.!!!",
            "FUNASR-eyJhIjoxfQ.short",
        ] {
            assert!(!verify(input).valid, "{input:?} must not verify");
        }
    }

    #[test]
    fn unset_key_fails_closed() {
        // With the placeholder key, even a well-formed licence must be
        // rejected — a build that forgot to set the key must not accept
        // everything.
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"a@b.c","issued":"2026-01-01"}"#);
        let signature = URL_SAFE_NO_PAD.encode([0u8; 64]);
        let info = verify(&format!("FUNASR-{payload}.{signature}"));
        assert!(!info.valid);
    }
}
