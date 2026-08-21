import Foundation
import KoushuCore

/// Licence verification, through the real Rust core.
///
/// This is the whole of slice 1's UniFFI surface, and it is here to prove the
/// seam rather than because licensing is urgent: it exercises a generated Swift
/// type, a generated error, and the version/checksum handshake with the compiled
/// library — on code that is small and already tested on the Rust side. Every
/// slice after this one arrives the same way, as an adapter in this target that
/// replaces a stub in `StubCore.swift`.
///
/// The adapter exists at all because the generated type is *not* the domain
/// type. `KoushuCore.LicenseInfo` is what the app is written against, and it
/// stays stable while UniFFI regenerates its own struct on every build; without
/// this translation, a field added in Rust would ripple into view code.
public struct RustLicenseService: LicenseService {
    public init() {
        // The generated bindings check the FFI contract version and a checksum
        // per exported function against the compiled library, and trap on a
        // mismatch. Doing it here means a stale `libkoushu_core.a` fails at
        // startup with a clear message rather than at the first verification —
        // which, for licensing, would be the least convenient possible moment.
        uniffiEnsureFunAsrCoreInitialized()
    }

    /// Note that this does not throw, and that this is the interesting part.
    ///
    /// A rejected licence is an *answer*, and the answer carries a sentence the
    /// user can act on. Modelling it as an error would force every platform to
    /// unwrap it back into the sentence it already was, and UniFFI's synthesised
    /// `errorDescription` would show a debug dump of the enum rather than the
    /// text inside it. The Rust side returns `LicenseInfo` for exactly this
    /// reason; this adapter is the proof that it survives the crossing.
    public func verify(_ license: String) -> KoushuCore.LicenseInfo {
        let info = verifyLicense(license: license)
        return KoushuCore.LicenseInfo(
            valid: info.valid,
            email: info.email,
            issued: info.issued,
            detail: info.detail
        )
    }
}
