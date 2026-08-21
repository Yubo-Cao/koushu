//! The parts of Fun ASR that are not a user interface.
//!
//! This crate exists so the macOS app can be written natively in SwiftUI while
//! Linux and Windows keep the Tauri shell, without the product being built
//! twice. Everything here is reached from Swift through UniFFI-generated
//! bindings and from the desktop shell as an ordinary Rust dependency.
//!
//! ## What belongs here
//!
//! Anything a second front-end would otherwise have to reimplement, and
//! anything whose behaviour must be identical on every platform: speech
//! recognition and its segmentation, storage and search, the LLM client,
//! licensing, and metering.
//!
//! ## What does not
//!
//! Anything the platform already answers better than we can — windows, panels,
//! hotkeys, tray icons, text injection. Those are deliberately left to the
//! shells. Pulling them in here would mean re-expressing AppKit through a C
//! ABI, which is precisely the hand-written `objc::msg_send!` code this split
//! exists to delete.
//!
//! ## Two rules for anything crossing the boundary
//!
//! **Results that are not failures stay data.** A rejected licence, a
//! transcription with low confidence, a hit-the-fair-use-cap — these are
//! answers, and they carry text the user can act on. Modelling them as errors
//! forces every platform to translate them back, and UniFFI's generated
//! `errorDescription` would show a debug dump rather than the sentence inside.
//!
//! **Long-running work is a stream with a cancel.** Transcription emits
//! partials before it commits, downloads emit progress, formatting streams
//! tokens. These are callback interfaces, not polling loops — a poll would put
//! the cadence policy in the UI, where the two platforms would immediately
//! disagree. And push-to-talk is released mid-utterance constantly, so
//! cancellation has to be expressible; dropping a future is not.
//!
//! See `docs/core-extraction.md` for the migration plan and its current state.

pub mod asr;
pub mod asr_cloud;
pub mod audio;
pub mod license;
pub mod llm;
pub mod storage;

uniffi::setup_scaffolding!();
