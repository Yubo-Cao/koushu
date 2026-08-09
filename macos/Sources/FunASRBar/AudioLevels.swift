import AVFoundation
import Foundation
import QuartzCore

/// Real microphone level, because a fake waveform looks fake and the bar's
/// credibility lives entirely in whether it reacts.
///
/// No ASR here. The shared Rust core is being extracted on another branch and
/// has no Swift package yet; this deliberately links nothing from it.
///
/// Two clocks on purpose. The audio tap runs on a render thread at the device's
/// buffer rate and does nothing but reduce a buffer to one number. A 60 Hz timer
/// on the main thread shapes the envelope and hands it to the view. Doing the
/// smoothing in the audio callback would tie the envelope's time constant to the
/// hardware buffer size.
@MainActor
final class AudioLevelMonitor {
    private let engine = AVAudioEngine()
    private var running = false
    private var displayTimer: Timer?
    private var startedAt: CFTimeInterval = 0

    /// Written on the audio thread, read on main. A double write/read is atomic
    /// enough for a meter — a torn sample would be one frame of a level bar.
    private var rawLevel: Double = 0
    private var envelope: Double = 0

    private weak var model: BarModel?

    /// Attack/release in "fraction of the gap closed per 60 Hz frame".
    /// Attack is nearly instantaneous so a consonant shows up on the frame it
    /// happens; release is slow enough that the meter doesn't strobe between
    /// syllables.
    private let attack = 0.6
    private let release = 0.12

    init(model: BarModel) {
        self.model = model
    }

    // MARK: Authorization

    func requestAccess(_ done: @escaping (Bool) -> Void) {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            done(true)
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { ok in
                DispatchQueue.main.async { done(ok) }
            }
        default:
            done(false)
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
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let self, let ch = buffer.floatChannelData else { return }
            let n = Int(buffer.frameLength)
            guard n > 0 else { return }
            var sum: Float = 0
            let samples = ch[0]
            for i in 0..<n { let s = samples[i]; sum += s * s }
            let rms = Double(sqrtf(sum / Float(n)))
            // -60 dBFS reads as silence, -15 dBFS as full scale. A linear
            // amplitude scale is useless here: ordinary speech sits around 1% of
            // full scale and would never leave the bottom pixel. Metering in dB
            // is what makes the top half of the bar reachable by a human voice.
            let db = 20 * log10(max(rms, 1e-7))
            let norm = max(0, min(1, (db + 60) / 45))
            self.rawLevel = norm
        }

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

    func stop() {
        guard running else { return }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        running = false
        displayTimer?.invalidate()
        displayTimer = nil
        rawLevel = 0
        envelope = 0
    }

    /// One history bar every `framesPerBar` envelope frames.
    ///
    /// Pushing a bar per 60 Hz frame looked wrong for a reason that is not
    /// obvious: 30 bars then cover half a second, so every pause between two
    /// syllables emptied the whole meter and it read as a row of dots. At 15 Hz
    /// the same 30 bars hold two seconds — a phrase rather than a fragment.
    private let framesPerBar = 4
    private var frameCount = 0
    /// Peak within the current bar's window. Averaging here would swallow exactly
    /// the transients that make speech look like speech.
    private var bucketPeak: Double = 0

    private func startDisplayTimer() {
        displayTimer?.invalidate()
        frameCount = 0
        bucketPeak = 0
        displayTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let model = self.model else { return }
                let target = self.rawLevel
                let k = target > self.envelope ? self.attack : self.release
                self.envelope += (target - self.envelope) * k
                // Expands the quiet end, the way a VU meter's scale does, so the
                // difference between a room tone and a whisper is visible
                // instead of both sitting on the floor.
                let shaped = pow(self.envelope, 0.7)

                self.bucketPeak = max(self.bucketPeak, shaped)
                self.frameCount += 1
                if self.frameCount >= self.framesPerBar {
                    model.push(level: CGFloat(self.bucketPeak))
                    self.frameCount = 0
                    self.bucketPeak = 0
                }
                model.level = CGFloat(shaped)
                model.elapsed = CACurrentMediaTime() - self.startedAt
            }
        }
    }
}
