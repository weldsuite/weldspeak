//! Sample-rate conversion for the capture path.
//!
//! Microphones hand back whatever they like — usually 44.1 or 48 kHz, often
//! stereo, usually f32 — and the recognizer wants exactly 16 kHz mono
//! `linear16`. This module does that conversion.
//!
//! The part that matters is the low-pass filter. Dropping samples to go from
//! 48 kHz to 16 kHz without filtering first folds everything above 8 kHz back
//! down into the speech band as aliasing: a 15 kHz hiss arrives as a 1 kHz tone
//! sitting on top of the vowels. It is inaudible to whoever wrote the code and
//! quietly wrecks recognition accuracy, so the filter is not optional.

/// Half-length of the anti-aliasing filter, in taps either side of centre.
///
/// 32 gives a transition band narrow enough to keep speech intact while
/// suppressing what would otherwise alias, at a cost of ~65 multiplies per
/// output sample — nothing next to the rest of the pipeline.
const HALF_TAPS: usize = 32;

/// Cutoff as a fraction of the output sample rate.
///
/// Nyquist for the output is 0.5. Backing off to 0.45 leaves room for the
/// filter's transition band, so content that would alias is well down before it
/// reaches the fold-over point. 7.2 kHz at 16 kHz output is comfortably above
/// the range that carries speech intelligibility.
const CUTOFF_FRACTION: f32 = 0.45;

/// Converts interleaved device audio to 16 kHz mono `i16`.
///
/// Holds the filter history between calls, so a stream fed in arbitrary chunk
/// sizes produces the same output as one fed in a single block. Without that,
/// every buffer boundary would be a discontinuity the recognizer hears as a click.
pub struct Resampler {
    input_rate: u32,
    output_rate: u32,
    channels: usize,
    taps: Vec<f32>,
    /// Filter history plus samples not yet consumed, at the input rate, mono.
    history: Vec<f32>,
    /// Fractional read position within `history`, in input samples.
    position: f32,
}

impl Resampler {
    /// Build a resampler from `input_rate`/`channels` to `output_rate` mono.
    pub fn new(input_rate: u32, channels: usize, output_rate: u32) -> Self {
        // When the rates match there is nothing to filter out, so the cutoff is
        // placed relative to whichever rate is lower — the one that decides
        // what has to be discarded.
        let limiting_rate = input_rate.min(output_rate) as f32;
        let cutoff = CUTOFF_FRACTION * limiting_rate / input_rate as f32;

        Self {
            input_rate,
            output_rate,
            channels: channels.max(1),
            taps: design_lowpass(cutoff),
            history: vec![0.0; HALF_TAPS * 2],
            position: HALF_TAPS as f32,
        }
    }

    /// Feed interleaved f32 samples and receive whatever output they produce.
    ///
    /// Output length varies between calls — the resampler emits only samples it
    /// can compute without reading past the end of its input.
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<i16> {
        self.history.extend(downmix(interleaved, self.channels));

        let step = self.input_rate as f32 / self.output_rate as f32;
        let mut output = Vec::new();

        // Stop while a full filter window still fits inside `history`.
        let limit = self.history.len().saturating_sub(HALF_TAPS) as f32;
        while self.position < limit {
            output.push(to_i16(self.filtered_at(self.position)));
            self.position += step;
        }

        self.discard_consumed();
        output
    }

    /// Flush the tail of the stream, padding so the last real samples emerge.
    ///
    /// Without this the final `HALF_TAPS` input samples never produce output and
    /// the last few milliseconds — often the end of the final word — are lost.
    pub fn flush(&mut self) -> Vec<i16> {
        self.history.extend(std::iter::repeat_n(0.0, HALF_TAPS));
        let tail = self.push(&[]);
        self.reset();
        tail
    }

    /// Clear all history, as when a new dictation starts on a different device.
    pub fn reset(&mut self) {
        self.history.clear();
        self.history.resize(HALF_TAPS * 2, 0.0);
        self.position = HALF_TAPS as f32;
    }

    /// One output sample: the filter applied at a fractional input position.
    ///
    /// The window is centred on the nearest input sample and the fractional part
    /// is handled by linear interpolation between neighbouring outputs. Because
    /// everything above the cutoff is already gone, that interpolation adds
    /// nothing audible.
    fn filtered_at(&self, position: f32) -> f32 {
        let centre = position.floor() as usize;
        let fraction = position - centre as f32;

        let left = self.convolve(centre);
        let right = self.convolve(centre + 1);

        left + (right - left) * fraction
    }

    fn convolve(&self, centre: usize) -> f32 {
        let mut sum = 0.0;
        for (offset, tap) in self.taps.iter().enumerate() {
            let index = centre + offset;
            if index >= HALF_TAPS {
                if let Some(sample) = self.history.get(index - HALF_TAPS) {
                    sum += sample * tap;
                }
            }
        }
        sum
    }

    /// Drop history the read position has moved past, keeping the filter window.
    fn discard_consumed(&mut self) {
        let keep_from = (self.position as usize).saturating_sub(HALF_TAPS);
        if keep_from > 0 {
            self.history.drain(..keep_from);
            self.position -= keep_from as f32;
        }
    }
}

/// Average channels down to mono.
///
/// Dictation is one speaker at one microphone; averaging is both correct and
/// half the bytes on the wire.
fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }

    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// A windowed-sinc low-pass with `cutoff` given as a fraction of the input rate.
///
/// The Hann window trades a little stopband depth for the absence of ringing,
/// which is the right trade for speech.
fn design_lowpass(cutoff: f32) -> Vec<f32> {
    let length = HALF_TAPS * 2 + 1;
    let mut taps = Vec::with_capacity(length);

    for index in 0..length {
        let offset = index as f32 - HALF_TAPS as f32;

        let sinc = if offset == 0.0 {
            2.0 * cutoff
        } else {
            let x = std::f32::consts::PI * offset;
            (2.0 * cutoff * x).sin() / x
        };

        let window = 0.5
            - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (length - 1) as f32).cos();

        taps.push(sinc * window);
    }

    // Normalize to unit DC gain so the conversion does not change loudness.
    let sum: f32 = taps.iter().sum();
    if sum.abs() > f32::EPSILON {
        for tap in &mut taps {
            *tap /= sum;
        }
    }

    taps
}

/// Convert to `i16`, clamping rather than wrapping.
///
/// A sample that overflows must saturate: wrapping turns a loud vowel into a
/// full-scale sign flip, which sounds like a gunshot and ruins the frame.
fn to_i16(sample: f32) -> i16 {
    (sample * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate `seconds` of a sine at `frequency`, interleaved across `channels`.
    fn tone(frequency: f32, rate: u32, seconds: f32, channels: usize) -> Vec<f32> {
        let frames = (rate as f32 * seconds) as usize;
        let mut samples = Vec::with_capacity(frames * channels);

        for frame in 0..frames {
            let value =
                (2.0 * std::f32::consts::PI * frequency * frame as f32 / rate as f32).sin() * 0.5;
            for _ in 0..channels {
                samples.push(value);
            }
        }
        samples
    }

    /// Root-mean-square amplitude, as a fraction of full scale.
    fn rms(samples: &[i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|s| (*s as f64).powi(2)).sum();
        (sum / samples.len() as f64).sqrt() as f32 / i16::MAX as f32
    }

    /// Estimate the dominant frequency by counting zero crossings.
    fn dominant_frequency(samples: &[i16], rate: u32) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|pair| (pair[0] >= 0) != (pair[1] >= 0))
            .count();

        crossings as f32 * rate as f32 / (2.0 * samples.len() as f32)
    }

    #[test]
    fn produces_roughly_the_expected_number_of_samples() {
        let mut resampler = Resampler::new(48_000, 1, 16_000);
        let output = resampler.push(&tone(440.0, 48_000, 1.0, 1));

        // One second in, one second out, give or take the filter's edges.
        assert!(
            (15_900..=16_100).contains(&output.len()),
            "expected ~16000 samples, got {}",
            output.len()
        );
    }

    #[test]
    fn preserves_a_speech_band_tone() {
        let mut resampler = Resampler::new(48_000, 1, 16_000);
        let output = resampler.push(&tone(1_000.0, 48_000, 0.5, 1));

        let frequency = dominant_frequency(&output, 16_000);
        assert!(
            (950.0..1_050.0).contains(&frequency),
            "expected ~1000 Hz, got {frequency}"
        );

        // And it should still be roughly as loud as it went in.
        assert!(rms(&output) > 0.25, "tone was attenuated: rms {}", rms(&output));
    }

    #[test]
    fn suppresses_content_that_would_otherwise_alias() {
        // 15 kHz cannot be represented at 16 kHz output. Undiscarded, it folds
        // down to 1 kHz and lands squarely in the speech band. This is the test
        // the anti-aliasing filter exists to pass.
        let mut resampler = Resampler::new(48_000, 1, 16_000);
        let output = resampler.push(&tone(15_000.0, 48_000, 0.5, 1));

        assert!(
            rms(&output) < 0.02,
            "15 kHz tone was not suppressed: rms {} (aliasing into the speech band)",
            rms(&output)
        );
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let mut mono = Resampler::new(48_000, 1, 16_000);
        let mut stereo = Resampler::new(48_000, 2, 16_000);

        let from_mono = mono.push(&tone(1_000.0, 48_000, 0.2, 1));
        let from_stereo = stereo.push(&tone(1_000.0, 48_000, 0.2, 2));

        assert_eq!(from_mono.len(), from_stereo.len());
        // The same signal in both channels averages back to itself.
        assert!((rms(&from_mono) - rms(&from_stereo)).abs() < 0.01);
    }

    #[test]
    fn chunked_input_matches_a_single_block() {
        // Buffer boundaries must not produce discontinuities: the filter history
        // is what makes a stream of small chunks equivalent to one big one.
        let input = tone(1_000.0, 48_000, 0.3, 1);

        let mut whole = Resampler::new(48_000, 1, 16_000);
        let single = whole.push(&input);

        let mut chunked = Resampler::new(48_000, 1, 16_000);
        let mut pieces = Vec::new();
        for chunk in input.chunks(577) {
            pieces.extend(chunked.push(chunk));
        }

        assert_eq!(single.len(), pieces.len());
        for (index, (a, b)) in single.iter().zip(&pieces).enumerate() {
            assert!(
                (a - b).abs() <= 1,
                "sample {index} differs: {a} vs {b}"
            );
        }
    }

    #[test]
    fn handles_a_rate_that_is_not_a_whole_multiple() {
        // 44.1 kHz is not an integer multiple of 16 kHz, which is exactly why
        // the read position is fractional.
        let mut resampler = Resampler::new(44_100, 1, 16_000);
        let output = resampler.push(&tone(1_000.0, 44_100, 0.5, 1));

        let frequency = dominant_frequency(&output, 16_000);
        assert!(
            (950.0..1_050.0).contains(&frequency),
            "expected ~1000 Hz from 44.1 kHz input, got {frequency}"
        );
    }

    #[test]
    fn passes_audio_through_when_the_rate_already_matches() {
        let mut resampler = Resampler::new(16_000, 1, 16_000);
        let output = resampler.push(&tone(1_000.0, 16_000, 0.5, 1));

        assert!(rms(&output) > 0.25);
        let frequency = dominant_frequency(&output, 16_000);
        assert!((950.0..1_050.0).contains(&frequency), "got {frequency}");
    }

    #[test]
    fn flush_emits_the_tail_and_resets() {
        let mut resampler = Resampler::new(48_000, 1, 16_000);
        resampler.push(&tone(1_000.0, 48_000, 0.1, 1));

        assert!(!resampler.flush().is_empty(), "flush should emit the tail");

        // After reset the next utterance starts from silence, not from the
        // previous one's history.
        assert_eq!(resampler.position, HALF_TAPS as f32);
    }

    #[test]
    fn silence_in_silence_out() {
        let mut resampler = Resampler::new(48_000, 1, 16_000);
        let output = resampler.push(&vec![0.0; 48_000]);

        assert!(output.iter().all(|sample| *sample == 0));
    }

    #[test]
    fn clamps_rather_than_wrapping_on_overload() {
        // A wrapped sample flips sign and sounds like a gunshot; a clamped one
        // merely distorts.
        assert_eq!(to_i16(2.0), i16::MAX);
        assert_eq!(to_i16(-2.0), i16::MIN);
    }
}
