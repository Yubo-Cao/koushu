//! Turning captured samples into the file the runtimes read.
//!
//! The two shells capture audio in completely different ways — `cpal` on Linux,
//! `AVAudioEngine` on macOS — and neither is better in the abstract, so capture
//! stays on their side of the boundary. What must not differ is what comes out
//! of it: the runtime's accuracy depends on the sample rate and the quantisation
//! of the file it is given, so if the two shells encoded their own WAVs they
//! would eventually disagree about how well transcription works, and the
//! difference would be invisible in both codebases.
//!
//! So this is the last step of capture and the first step of recognition, and it
//! belongs to whichever side can only have one copy of it.

/// 16 kHz mono is what the Fun-ASR models were trained on. The runtimes will
/// resample anything else themselves, but doing it once here is cheaper and
/// makes the file that reached the runtime reproducible from the file we kept.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// How loud a buffer is, on the three scales the interfaces use.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct AudioLevel {
    pub rms: f32,
    pub peak: f32,
    pub db: f32,
    /// 0–100, for a progress-style meter. -60 dBFS is the floor.
    pub percent: f32,
}

/// What a finished recording amounts to.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CaptureSummary {
    pub duration_ms: f64,
    pub level: AudioLevel,
    /// Whether this is worth sending to a recogniser at all.
    ///
    /// Both halves matter. A recording under ~650 ms is a key-press, not a
    /// sentence, and transcribing it produces a confident hallucination. A
    /// recording that never left the noise floor is a room, and the honest
    /// response is "no speech detected" rather than whatever the decoder
    /// invents from silence.
    pub speech_like: bool,
}

pub fn summarize(samples: &[f32], sample_rate: u32) -> CaptureSummary {
    let level = level_of(samples);
    let duration_ms = if sample_rate > 0 {
        samples.len() as f64 / f64::from(sample_rate) * 1000.0
    } else {
        0.0
    };
    CaptureSummary {
        duration_ms,
        level,
        speech_like: duration_ms >= 650.0 && (level.rms >= 0.006 || level.peak >= 0.025),
    }
}

pub fn level_of(samples: &[f32]) -> AudioLevel {
    if samples.is_empty() {
        return AudioLevel {
            rms: 0.0,
            peak: 0.0,
            db: -90.0,
            percent: 0.0,
        };
    }

    let mut sum_squares = 0.0_f64;
    let mut peak = 0.0_f32;
    for sample in samples {
        let value = sample.abs().min(1.0);
        peak = peak.max(value);
        sum_squares += f64::from(value * value);
    }
    let rms = (sum_squares / samples.len() as f64).sqrt() as f32;
    let db = if rms > 0.0 {
        (20.0 * rms.max(0.000_031_6).log10()).max(-90.0)
    } else {
        -90.0
    };

    AudioLevel {
        rms,
        peak,
        db,
        percent: ((db + 60.0) / 60.0 * 100.0).clamp(0.0, 100.0),
    }
}

/// Resample, encode and write, in one call across the FFI.
///
/// One call rather than three because each one copies the samples: a minute of
/// 48 kHz audio is 11 MB, and the interesting number is how many times that
/// crosses the boundary. Returns what was recorded, so the caller can decide
/// whether to bother transcribing it without walking the buffer again.
#[uniffi::export]
pub fn write_capture_wav(
    samples: Vec<f32>,
    source_sample_rate: u32,
    path: String,
) -> Result<CaptureSummary, AudioError> {
    let summary = summarize(&samples, source_sample_rate);
    let resampled = resample_linear(&samples, source_sample_rate, TARGET_SAMPLE_RATE);
    let wav = encode_wav_i16(&resampled, TARGET_SAMPLE_RATE)?;
    std::fs::write(&path, wav).map_err(|err| AudioError::Write {
        detail: format!("Could not write the recording to {path}: {err}"),
    })?;
    Ok(summary)
}

/// The one thing here that can genuinely fail, as opposed to being an answer.
///
/// Unlike a rejected licence or a silent recording, a disk that will not take
/// the file is not a result the user can act on by changing what they said.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum AudioError {
    #[error("{detail}")]
    TooLarge { detail: String },
    #[error("{detail}")]
    Write { detail: String },
}

/// Linear interpolation.
///
/// Not a windowed-sinc: the input is speech being handed to a model that
/// resamples internally anyway, the ratio is usually 48k→16k, and the aliasing
/// this admits sits above the band the encoder looks at. A better resampler here
/// would be measurable in a test and inaudible to the recogniser.
pub fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || from_rate == to_rate {
        return samples.to_vec();
    }

    let output_len =
        (((samples.len() as f64) * (f64::from(to_rate) / f64::from(from_rate))).round() as usize)
            .max(1);
    let ratio = f64::from(from_rate) / f64::from(to_rate);
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let source = index as f64 * ratio;
        let left = (source.floor() as usize).min(samples.len() - 1);
        let right = (left + 1).min(samples.len() - 1);
        let frac = (source - left as f64) as f32;
        output.push((samples[left] * (1.0 - frac) + samples[right] * frac).clamp(-1.0, 1.0));
    }

    output
}

/// 16-bit PCM, mono, with a 44-byte canonical header.
pub fn encode_wav_i16(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, AudioError> {
    let too_large = || AudioError::TooLarge {
        detail: "The recording is too large to encode.".to_string(),
    };

    let data_len = samples.len().checked_mul(2).ok_or_else(too_large)?;
    let riff_len = 36_usize.checked_add(data_len).ok_or_else(too_large)?;
    if riff_len > u32::MAX as usize {
        return Err(too_large());
    }

    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(riff_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2_u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());

    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        wav.extend_from_slice(&value.to_le_bytes());
    }

    Ok(wav)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_says_what_the_runtimes_expect() {
        let wav = encode_wav_i16(&[0.0; 160], 16_000).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            16_000
        );
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16, "16-bit");
        assert_eq!(wav.len(), 44 + 320);
    }

    #[test]
    fn resampling_48k_to_16k_gives_a_third_of_the_samples() {
        let input = vec![0.5_f32; 4_800];
        assert_eq!(resample_linear(&input, 48_000, 16_000).len(), 1_600);
    }

    #[test]
    fn a_matching_rate_is_a_passthrough_not_a_pass_over_the_interpolator() {
        let input = vec![0.1, -0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn silence_is_not_speech_and_neither_is_a_keypress() {
        // A quiet room, long enough to be a sentence.
        let quiet = summarize(&vec![0.0005_f32; 32_000], 16_000);
        assert!(quiet.duration_ms > 650.0);
        assert!(!quiet.speech_like, "room tone should not be transcribed");

        // Loud, but 100 ms — somebody tapping the key.
        let brief = summarize(&vec![0.4_f32; 1_600], 16_000);
        assert!(!brief.speech_like, "a keypress should not be transcribed");

        let speech = summarize(&vec![0.2_f32; 32_000], 16_000);
        assert!(speech.speech_like);
    }

    #[test]
    fn clipping_is_clamped_rather_than_wrapped() {
        // Without the clamp this wraps to -32768 and the loudest moment of a
        // recording becomes its quietest.
        let wav = encode_wav_i16(&[2.0], 16_000).unwrap();
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), i16::MAX);
    }
}
