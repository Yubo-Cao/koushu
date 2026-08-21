import AVFoundation
import AppKit
import Foundation
import KoushuCore
import QuartzCore

/// Real microphone level.
///
/// The bar's credibility lives entirely in whether it reacts, so this is the one
/// thing in the app that is never simulated: it opens the actual input device
/// and reduces actual buffers. It does no recognition — that belongs to the
/// core, and the core does not have it yet.
///
/// Two clocks on purpose. The audio tap runs on a render thread at the device's
/// buffer rate and does nothing but reduce a buffer to one number. A 60 Hz timer
/// on the main thread shapes the envelope and hands it to the view. Doing the
/// smoothing in the audio callback would tie the envelope's time constant to the
/// hardware buffer size, so the meter would behave differently on a device with
/// a different buffer.
@MainActor
final class AudioLevelMonitor {
    private let engine = AVAudioEngine()
    private var running = false
    private var displayTimer: Timer?
    private var startedAt: CFTimeInterval = 0

    /// The rate the device actually gave us, which is not necessarily the rate
    /// anyone asked for — 48 kHz on built-in hardware, 16 kHz on some headsets.
    /// The recording is resampled from this, so it has to be recorded alongside.
    private(set) var captureSampleRate: Double = 0

    /// Everything that crosses from the render thread to the main thread.
    ///
    /// A separate object, and the *only* thing the tap closure captures. Having
    /// the tap capture `self` instead is not merely untidy: this class is
    /// `@MainActor`, so under Swift 6 every touch of a stored property through
    /// `self` compiles to an executor check, and the audio render thread is not
    /// the main executor. The process does not misbehave, it traps —
    /// `dispatch_assert_queue` → `SIGTRAP`, on the first buffer, every time.
    ///
    /// (The prototype had exactly this shape and appeared to work, because it
    /// was compiled in Swift 5 language mode where no check is emitted. It was
    /// always undefined behaviour; Swift 6 turned it into a crash on the first
    /// recording, which is how it was found.)
    private let sink = CaptureSink()
    private var envelope: Double = 0

    private weak var model: AppModel?

    /// Attack/release as "fraction of the gap closed per 60 Hz frame". Attack is
    /// nearly instantaneous so a consonant shows up on the frame it happens;
    /// release is slow enough that the meter does not strobe between syllables.
    private let attack = 0.6
    private let release = 0.12

    init(model: AppModel) {
        self.model = model
    }

    // MARK: Authorization

    /// The current answer, without asking. Asking puts a dialog on screen.
    static var authorization: AVAuthorizationStatus {
        AVCaptureDevice.authorizationStatus(for: .audio)
    }

    /// Ask, if nobody has been asked yet.
    ///
    /// Only ever called from something that is about to record. A permission
    /// prompt on launch is a prompt for a thing the user has not done.
    func requestAccess() async -> Bool {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            return true
        case .notDetermined:
            return await AVCaptureDevice.requestAccess(for: .audio)
        default:
            return false
        }
    }

    static func openMicrophoneSettings() {
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        else { return }
        NSWorkspace.shared.open(url)
    }

    /// The input devices macOS is offering.
    ///
    /// `AVCaptureDevice.DiscoverySession` rather than the Core Audio device
    /// list: it is the same set the system's own microphone picker shows, so a
    /// device named here is a device the user can recognise.
    static func inputs() -> [AudioInputInfo] {
        let session = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.microphone, .external],
            mediaType: .audio,
            position: .unspecified
        )
        let systemDefault = AVCaptureDevice.default(for: .audio)
        return session.devices.map { device in
            AudioInputInfo(
                id: device.uniqueID,
                name: device.localizedName,
                isDefault: device.uniqueID == systemDefault?.uniqueID
            )
        }
    }

    // MARK: Engine

    func start() {
        guard !running else { return }
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            NSLog("[audio] input node has no usable format: \(format)")
            return
        }

        input.removeTap(onBus: 0)
        // Captures `sink` and nothing else — see the note on `CaptureSink`.
        let sink = self.sink
        captureSampleRate = format.sampleRate
        sink.beginCollecting(expectedRate: format.sampleRate)
        // `@Sendable` is load-bearing, not decoration. `AVAudioNodeTapBlock`
        // carries no Sendable annotation, so a closure written inline here
        // *inherits the isolation of where it was written* — this method, which
        // is `@MainActor`. Swift then emits an executor check at the closure's
        // entry, the render thread fails it, and the process traps on the first
        // buffer. Marking the closure `@Sendable` is what makes it nonisolated,
        // and the reason that is safe is the line above: it touches one lock-
        // guarded box and nothing else.
        let tap: AVAudioNodeTapBlock = { @Sendable buffer, _ in
            guard let channels = buffer.floatChannelData else { return }
            let count = Int(buffer.frameLength)
            guard count > 0 else { return }
            var sum: Float = 0
            let samples = channels[0]
            for index in 0..<count {
                let sample = samples[index]
                sum += sample * sample
            }
            let rms = Double(sqrtf(sum / Float(count)))
            // -60 dBFS reads as silence, -15 dBFS as full scale. A linear
            // amplitude scale is useless here: ordinary speech sits around 1% of
            // full scale and would never leave the bottom pixel. Metering in dB
            // is what makes the top half of the meter reachable by a voice.
            let db = 20 * log10(max(rms, 1e-7))
            sink.level = max(0, min(1, (db + 60) / 45))
            sink.append(samples, count: count)
        }
        input.installTap(onBus: 0, bufferSize: 1024, format: format, block: tap)

        engine.prepare()
        do {
            try engine.start()
            running = true
            startedAt = CACurrentMediaTime()
            startDisplayTimer()
            NSLog("[audio] engine started, format=\(format)")
        } catch {
            NSLog("[audio] engine failed to start: \(error)")
        }
    }

    /// Stop, and hand back what was recorded.
    ///
    /// The samples come out here rather than being read separately, because the
    /// only safe moment to take them is the one where the tap has already been
    /// removed — after that nothing can append, and the buffer is final.
    @discardableResult
    func stop() -> (samples: [Float], sampleRate: Double) {
        guard running else { return ([], captureSampleRate) }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        running = false
        displayTimer?.invalidate()
        displayTimer = nil
        let recorded = sink.takeSamples()
        sink.level = 0
        envelope = 0
        model?.level = 0
        return (recorded, captureSampleRate)
    }

    /// The most recent shaped level, in dB, for the numeric readout beside the
    /// meter. Derived from the same envelope the bars are drawn from, so the
    /// number and the picture can never disagree.
    private(set) var decibels: Double = -90

    /// One history bar every `framesPerBar` envelope frames.
    ///
    /// Pushing a bar per 60 Hz frame looked wrong for a reason that is not
    /// obvious: 30 bars then cover half a second, so every pause between two
    /// syllables emptied the whole meter and it read as a row of dots. At 15 Hz
    /// the same 30 bars hold two seconds — a phrase rather than a fragment.
    private let framesPerBar = 4
    private var frameCount = 0
    /// Peak within the current bar's window. Averaging here would swallow
    /// exactly the transients that make speech look like speech.
    private var bucketPeak: Double = 0

    private func startDisplayTimer() {
        displayTimer?.invalidate()
        frameCount = 0
        bucketPeak = 0
        displayTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let model = self.model else { return }
                let target = self.sink.level
                let coefficient = target > self.envelope ? self.attack : self.release
                self.envelope += (target - self.envelope) * coefficient
                // Expands the quiet end the way a VU meter's scale does, so the
                // difference between room tone and a whisper is visible instead
                // of both sitting on the floor.
                let shaped = pow(self.envelope, 0.7)

                self.bucketPeak = max(self.bucketPeak, shaped)
                self.frameCount += 1
                if self.frameCount >= self.framesPerBar {
                    model.push(level: CGFloat(self.bucketPeak))
                    self.frameCount = 0
                    self.bucketPeak = 0
                }
                self.decibels = self.envelope > 0 ? (self.envelope * 45) - 60 : -90
                model.level = CGFloat(shaped)
                model.elapsed = CACurrentMediaTime() - self.startedAt
            }
        }
    }
}

/// Everything that crosses from the audio render thread to the main thread.
///
/// A lock rather than bare `var`s: the writes are on a real-time thread and the
/// reads are on the main one, and while a torn `Double` would only ever be one
/// wrong frame of a level bar, "only one frame" is not a memory model. It is
/// uncontended in practice — one writer, one reader, each holding it briefly —
/// so it costs nothing a render thread would notice.
///
/// The sample buffer is here for the same reason and one more: it has to be
/// *appended to* from the render thread and *taken whole* from the main one, and
/// those two cannot overlap without losing audio.
final class CaptureSink: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: Double = 0
    private var samples: [Float] = []
    private var collecting = false

    /// The most recent normalised level, 0...1.
    var level: Double {
        get { lock.withLock { storage } }
        set { lock.withLock { storage = newValue } }
    }

    /// Start keeping the audio, not just measuring it.
    ///
    /// Reserves a minute up front. A `reserveCapacity` on the main thread costs
    /// nothing; the same growth happening inside the tap would be a malloc on a
    /// real-time thread, which is the one thing that is genuinely not allowed
    /// there.
    func beginCollecting(expectedRate: Double) {
        lock.withLock {
            samples.removeAll(keepingCapacity: true)
            samples.reserveCapacity(Int(expectedRate * 60))
            collecting = true
        }
    }

    func append(_ buffer: UnsafePointer<Float>, count: Int) {
        lock.withLock {
            guard collecting else { return }
            samples.append(contentsOf: UnsafeBufferPointer(start: buffer, count: count))
        }
    }

    /// Take the recording and stop collecting, in one step so the render thread
    /// cannot append to a buffer that has already been handed over.
    func takeSamples() -> [Float] {
        lock.withLock {
            collecting = false
            defer { samples.removeAll(keepingCapacity: true) }
            return samples
        }
    }
}
