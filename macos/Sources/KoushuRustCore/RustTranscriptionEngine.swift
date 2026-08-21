import Foundation
import KoushuCore

/// Real speech recognition, through the official Fun-ASR llama.cpp runtimes.
///
/// The work happens in `koushu-core`; this is the adapter. Two things it has to
/// get right, and neither is about recognition:
///
/// **It must not run on the main thread.** `AsrJob.run` blocks until the runtime
/// exits, which for Fun-ASR-Nano is seconds. Called from the main actor it would
/// freeze the voice bar, the menu-bar icon and every window for the whole decode
/// — while the one thing the user is watching is the bar, waiting for words.
///
/// **Cancellation has to reach the process.** `Task.cancel()` on the Swift side
/// stops nothing: the thread is inside a blocking FFI call. So cancelling calls
/// through to `AsrJob.cancel`, which kills the child. That is also why the job
/// object is created before the work starts rather than inside it.
public struct RustTranscriptionEngine: TranscriptionEngine {
    private let runtime: AsrRuntimePaths
    private let modelDirectory: @Sendable (String) -> String

    /// - Parameters:
    ///   - nanoCLI: path to `llama-funasr-cli`.
    ///   - senseVoiceCLI: path to `llama-funasr-sensevoice`.
    ///   - modelDirectory: maps a model id to the directory holding its GGUF
    ///     files. Injected because where models live is a question about the
    ///     platform's directory layout, which the core has no business knowing.
    public init(
        nanoCLI: String,
        senseVoiceCLI: String,
        modelDirectory: @escaping @Sendable (String) -> String
    ) {
        self.runtime = AsrRuntimePaths(nanoCli: nanoCLI, sensevoiceCli: senseVoiceCLI)
        self.modelDirectory = modelDirectory
    }

    public func missingAssets(modelID: String, backend: String) -> [String] {
        KoushuRustCore.missingAssets(backend: backend, modelDir: modelDirectory(modelID))
    }

    public func transcribe(
        _ request: TranscriptionRequest,
        onEvent: @escaping @Sendable (TranscriptionEvent) -> Void
    ) -> CoreCancellable {
        let job = AsrJob()
        let runtime = self.runtime
        let modelDir = modelDirectory(request.modelID)

        // `.detached` and not `Task {}`: an unstructured task inherits the
        // actor context it was created in, and this one is created from the
        // main actor. Inheriting it would put a multi-second blocking call on
        // the main thread, which is exactly what this must not do.
        Task.detached(priority: .userInitiated) {
            let outcome = job.run(
                runtime: runtime,
                request: AsrRequest(
                    backend: request.backend,
                    modelDir: modelDir,
                    wavPath: request.wavPath
                )
            )

            // Cancelled is not failed. The user let go, or changed their mind;
            // there is nothing to report and nothing to store.
            if outcome.cancelled { return }

            if let failure = outcome.failure {
                onEvent(.failed(failure))
                return
            }

            let text = outcome.text.trimmingCharacters(in: .whitespacesAndNewlines)
            // The runtime ran, and printed nothing. That means the VAD found no
            // speech — a different thing from a decode that failed, and the two
            // send the user to look at different places.
            if text.isEmpty {
                onEvent(.noSpeech)
                return
            }

            onEvent(.committed(text: text, elapsedMS: Int(outcome.elapsedMs)))
        }

        return AsrJobCancellable(job: job)
    }
}

/// Writes a recording where the runtime can read it.
///
/// The encoder is in the core rather than here so both shells hand the runtime
/// byte-identical input: transcription accuracy depends on the sample rate and
/// quantisation of this file, and two independently-written WAV writers would
/// eventually differ in a way that showed up as "recognition is worse on macOS"
/// with nothing in either codebase to point at.
public enum RustAudioWriter {
    /// - Returns: how long the recording was and whether it is worth decoding,
    ///   or `nil` if it could not be written at all.
    public static func write(
        samples: [Float],
        sampleRate: Double,
        to path: String
    ) -> (durationMS: Double, speechLike: Bool)? {
        do {
            let summary = try writeCaptureWav(
                samples: samples,
                sourceSampleRate: UInt32(max(0, sampleRate)),
                path: path
            )
            return (summary.durationMs, summary.speechLike)
        } catch {
            NSLog("[audio] could not write the recording: \(error)")
            return nil
        }
    }
}

final class AsrJobCancellable: CoreCancellable, @unchecked Sendable {
    private let job: AsrJob
    init(job: AsrJob) { self.job = job }
    func cancel() { job.cancel() }
}
