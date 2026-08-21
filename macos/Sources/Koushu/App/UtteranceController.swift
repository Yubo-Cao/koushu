import AppKit
import Foundation
import KoushuCore

/// One press-and-hold, from the key going down to the text arriving somewhere.
///
/// This is the part of the app that has to be exactly right, because everything
/// it touches belongs to somebody else: the microphone, the keyboard, and the
/// document the words end up in. The order is deliberate —
///
///   1. capture the focused application **first**, before anything can steal it;
///   2. open the microphone;
///   3. on release, write the recording and hand it to the core;
///   4. deliver the result to the application captured in step 1 — not to
///      whatever happens to be frontmost when the words finally arrive.
@MainActor
final class UtteranceController {
    private let app: AppModel
    private let audio: AudioLevelMonitor
    private let browser: SessionBrowser

    private var decoding: (any CoreCancellable)?
    private var target: InjectTarget?
    private var sessionID: String?
    private var recording = false
    /// The WAV for the utterance in flight. Deleted once decoded, unless the
    /// user asked for recordings to be retained.
    private var wavURL: URL?

    /// Whether this utterance is allowed to type its result into another app.
    ///
    /// False for anything the control channel started. That channel exists so
    /// the app can be driven from a shell for screenshots and checks, and an
    /// utterance started that way is a rehearsal — nobody dictated it, nobody is
    /// waiting for the words, and the "focused application" is whatever the
    /// person at the machine happens to be reading.
    ///
    /// This was found the hard way: a scripted record/stop cycle posted 47
    /// characters of placeholder text into the user's chat window.
    private var deliversText = false

    init(app: AppModel, audio: AudioLevelMonitor, browser: SessionBrowser) {
        self.app = app
        self.audio = audio
        self.browser = browser
    }

    var isBusy: Bool { recording || decoding != nil }

    // MARK: Begin

    /// - Parameter deliversText: only ever true for a real push-to-talk press.
    func begin(deliversText: Bool) {
        guard !isBusy else { return }
        self.deliversText = deliversText
        // Before the panel appears and before anything else runs: whatever has
        // the keyboard right now is where the words belong.
        target = .current

        Task { await start() }
    }

    private func start() async {
        let granted = await audio.requestAccess()
        app.microphoneGranted = granted
        guard granted else {
            app.coreMessage = app.l(.micNeedsPermission)
            return
        }

        // Asked before recording, not after. A missing GGUF turns into a
        // sentence on screen instead of a recording that turns out to have been
        // pointless — and the user has already spoken by then.
        let missing = app.core.transcription.missingAssets(
            modelID: app.defaultModelID,
            backend: app.runtime
        )
        guard missing.isEmpty else {
            app.coreMessage = app.locale == .zh
                ? "模型文件缺失：\(missing.joined(separator: "、"))。请到设置里重新下载。"
                : "Missing model files: \(missing.joined(separator: ", ")). Download the model again in Settings."
            return
        }

        guard let session = await browser.ensureSession(
            untitled: app.l(.sessionUntitled),
            model: app.defaultModelID,
            language: app.defaultLanguage,
            runtime: app.runtime
        ) else {
            app.coreMessage = "Could not open a session to record into."
            return
        }
        sessionID = session.id

        recording = true
        app.beginRecording()
        audio.start()
    }

    // MARK: End

    /// The key came up. The whole recording is decoded from here, in one pass.
    func finish() {
        guard recording else { return }
        recording = false

        let capture = audio.stop()
        app.beginTranscribing()

        guard !capture.samples.isEmpty, capture.sampleRate > 0 else {
            app.abandon()
            return
        }

        let url = AppPaths.scratchAudioDirectory
            .appendingPathComponent("utterance-\(UUID().uuidString).wav")
        wavURL = url

        guard let written = AudioWriter.write(
            samples: capture.samples,
            sampleRate: capture.sampleRate,
            to: url.path
        ) else {
            app.abandon()
            app.coreMessage = app.locale == .zh
                ? "录音没能写到磁盘上。"
                : "The recording could not be written to disk."
            return
        }

        // A recording under ~650 ms is a key-press, not a sentence, and one that
        // never left the noise floor is a room. Transcribing either produces a
        // confident invention, so neither is sent.
        guard written.speechLike else {
            cleanUp()
            app.abandon()
            app.coreMessage = app.locale == .zh ? "没听到说话。" : "No speech detected."
            return
        }

        decoding = app.core.transcription.transcribe(
            TranscriptionRequest(
                wavPath: url.path,
                modelID: app.defaultModelID,
                backend: app.runtime,
                language: app.defaultLanguage
            ),
            onEvent: { [weak self] event in
                Task { @MainActor in self?.receive(event, durationMS: written.durationMS) }
            }
        )
    }

    /// Abandon it. Nothing is stored and nothing is typed.
    func cancel() {
        recording = false
        decoding?.cancel()
        decoding = nil
        audio.stop()
        cleanUp()
        app.abandon()
    }

    // MARK: Events

    private func receive(_ event: TranscriptionEvent, durationMS: Double) {
        switch event {
        case .partial(_, let text):
            app.partial = text

        case .committed(let text, let elapsedMS):
            decoding = nil
            NSLog("[asr] \(text.count) characters in \(elapsedMS) ms of runtime")
            Task { await save(text, durationMS: durationMS) }
            app.finish(with: text)
            deliver(text)
            cleanUp()

        case .noSpeech:
            decoding = nil
            cleanUp()
            app.abandon()
            app.coreMessage = app.locale == .zh ? "没听到说话。" : "No speech detected."

        case .failed(let message):
            decoding = nil
            cleanUp()
            app.abandon()
            // Verbatim. The core's failures already carry a sentence the user
            // can act on; translating them here would mean writing each one
            // twice and keeping both in step.
            app.coreMessage = message
        }
    }

    private func save(_ text: String, durationMS: Double) async {
        guard let sessionID else { return }
        let transcript = await browser.appendTranscript(
            sessionID: sessionID,
            text: text,
            model: app.defaultModelID,
            language: app.defaultLanguage,
            durationMS: Int(durationMS)
        )
        if transcript == nil {
            app.coreMessage = app.locale == .zh
                ? "转写成功，但没能存进历史记录。"
                : "Transcribed, but could not be saved to history."
        }
    }

    /// Remove the recording unless the user asked to keep it.
    private func cleanUp() {
        guard let url = wavURL else { return }
        wavURL = nil
        guard !app.retainAudio else { return }
        try? FileManager.default.removeItem(at: url)
    }

    // MARK: Delivery

    private func deliver(_ text: String) {
        guard deliversText else {
            NSLog("[inject] skipped: this utterance was not started by the hotkey")
            return
        }
        guard app.autoInsert, !text.isEmpty else { return }

        // On the clipboard as well as into the app. An insertion that is refused
        // — no Accessibility grant, or the user switched windows mid-sentence —
        // would otherwise lose the transcript entirely, and it is the one thing
        // here that cannot be reproduced by trying again.
        TextInjector.copyToPasteboard(text)

        if let refusal = TextInjector.insert(text, into: target) {
            switch refusal {
            case .notTrusted:
                app.coreMessage = app.l(.hotkeyNeedsAccessibility)
            case .targetChanged(let from, let to):
                app.coreMessage = app.locale == .zh
                    ? "焦点从 \(from) 移到了 \(to)，没有插入文字。内容已复制到剪贴板。"
                    : "Focus moved from \(from) to \(to), so nothing was typed. The transcript is on the clipboard."
            }
        }
    }
}
