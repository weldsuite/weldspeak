//! Audio format constants.
//!
//! These mirror `packages/protocol/src/audio.ts` and must stay in step with it;
//! the constants are asserted against each other in the Worker test suite.

/// Sample rate sent upstream, in Hz.
pub const SAMPLE_RATE: u32 = 16_000;

/// Wire encoding understood by the upstream model: little-endian signed 16-bit PCM.
pub const ENCODING: &str = "linear16";

/// Channel count. Dictation is single-speaker, so mono halves the bytes on the wire.
pub const CHANNELS: u16 = 1;

/// Bytes per sample for `linear16`.
pub const BYTES_PER_SAMPLE: usize = 2;

/// Duration of one audio frame, in milliseconds.
pub const FRAME_MS: u32 = 20;

/// Samples in one frame.
pub const FRAME_SAMPLES: usize = (SAMPLE_RATE as usize * FRAME_MS as usize) / 1000;

/// Bytes in one frame.
pub const FRAME_BYTES: usize = FRAME_SAMPLES * BYTES_PER_SAMPLE * CHANNELS as usize;

/// Pre-roll retained ahead of the hotkey press, in milliseconds.
///
/// People start speaking fractionally before the key is fully down, so the
/// capture thread keeps a rolling buffer and prepends it. Without this the
/// first phoneme is clipped and the model guesses at it. 500 ms covers a slow
/// finger without retaining so much silence that the recognizer stalls.
pub const PREROLL_MS: u32 = 500;

/// Frames held in the pre-roll ring buffer.
pub const PREROLL_FRAMES: usize = (PREROLL_MS / FRAME_MS) as usize;

/// Duration in milliseconds of `bytes` of `linear16` audio.
pub fn bytes_to_ms(bytes: usize) -> u64 {
    (bytes as u64 * 1000) / (SAMPLE_RATE as u64 * BYTES_PER_SAMPLE as u64 * CHANNELS as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_geometry_matches_twenty_milliseconds() {
        assert_eq!(FRAME_SAMPLES, 320);
        assert_eq!(FRAME_BYTES, 640);
        assert_eq!(PREROLL_FRAMES, 25);
    }

    #[test]
    fn one_second_of_audio_reports_one_second() {
        let one_second = SAMPLE_RATE as usize * BYTES_PER_SAMPLE;
        assert_eq!(bytes_to_ms(one_second), 1000);
    }
}
