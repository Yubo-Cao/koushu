import Foundation

#if KOUSHU_HAS_RUST_CORE
import KoushuRustCore
#endif

/// Writes a recording as the 16 kHz mono WAV the runtimes read.
///
/// Two implementations, and the split matters. With the Rust core linked, the
/// encoder in `koushu-core::audio` is used, so this app and the Tauri build
/// hand the runtime byte-identical input — recognition quality depends on the
/// sample rate and quantisation of this file, and two independently written WAV
/// writers would eventually differ in a way that showed up as "it recognises
/// worse on macOS" with nothing in either codebase to point at.
///
/// Without the core there is nothing to transcribe with anyway, so the fallback
/// exists only so the capture path can be exercised and the file inspected. It
/// is deliberately the same arithmetic, not a better resampler: a fallback that
/// produced *different* audio would make "does it sound right?" an unanswerable
/// question about which build made the file.
enum AudioWriter {
    struct Written {
        var durationMS: Double
        /// Whether this is worth decoding at all. A recording under ~650 ms is
        /// a key-press rather than a sentence, and one that never left the noise
        /// floor is a room; both make a decoder invent something confident.
        var speechLike: Bool
    }

    static func write(samples: [Float], sampleRate: Double, to path: String) -> Written? {
        #if KOUSHU_HAS_RUST_CORE
        guard let result = RustAudioWriter.write(samples: samples, sampleRate: sampleRate, to: path)
        else { return nil }
        return Written(durationMS: result.durationMS, speechLike: result.speechLike)
        #else
        return writeLocally(samples: samples, sampleRate: sampleRate, to: path)
        #endif
    }

    #if !KOUSHU_HAS_RUST_CORE
    private static func writeLocally(samples: [Float], sampleRate: Double, to path: String) -> Written? {
        guard sampleRate > 0, !samples.isEmpty else { return nil }
        let target: Double = 16_000
        let resampled = resample(samples, from: sampleRate, to: target)

        var peak: Float = 0
        var sumSquares = 0.0
        for sample in resampled {
            let value = min(abs(sample), 1)
            peak = max(peak, value)
            sumSquares += Double(value * value)
        }
        let rms = Float((sumSquares / Double(resampled.count)).squareRoot())
        let durationMS = Double(samples.count) / sampleRate * 1000

        var wav = Data()
        let dataLength = UInt32(resampled.count * 2)
        func append<T: FixedWidthInteger>(_ value: T) {
            withUnsafeBytes(of: value.littleEndian) { wav.append(contentsOf: $0) }
        }
        wav.append(contentsOf: Array("RIFF".utf8))
        append(UInt32(36) + dataLength)
        wav.append(contentsOf: Array("WAVEfmt ".utf8))
        append(UInt32(16))
        append(UInt16(1))                       // PCM
        append(UInt16(1))                       // mono
        append(UInt32(target))
        append(UInt32(target) * 2)              // byte rate
        append(UInt16(2))                       // block align
        append(UInt16(16))                      // bits per sample
        wav.append(contentsOf: Array("data".utf8))
        append(dataLength)
        for sample in resampled {
            append(Int16((max(-1, min(1, sample)) * Float(Int16.max)).rounded()))
        }

        do {
            try wav.write(to: URL(fileURLWithPath: path))
        } catch {
            NSLog("[audio] could not write the recording: \(error)")
            return nil
        }
        return Written(
            durationMS: durationMS,
            speechLike: durationMS >= 650 && (rms >= 0.006 || peak >= 0.025)
        )
    }

    private static func resample(_ samples: [Float], from: Double, to: Double) -> [Float] {
        guard from != to else { return samples }
        let count = max(1, Int((Double(samples.count) * to / from).rounded()))
        let ratio = from / to
        var output = [Float]()
        output.reserveCapacity(count)
        for index in 0..<count {
            let source = Double(index) * ratio
            let left = min(Int(source), samples.count - 1)
            let right = min(left + 1, samples.count - 1)
            let fraction = Float(source - Double(left))
            output.append(samples[left] * (1 - fraction) + samples[right] * fraction)
        }
        return output
    }
    #endif
}
