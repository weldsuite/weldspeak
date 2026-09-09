//! Framing and the pre-roll buffer.
//!
//! Audio arrives from the device in whatever block size it feels like and has
//! to leave as fixed 20 ms frames. This module does that, and holds the
//! pre-roll.
//!
//! The pre-roll matters more than it looks. People begin speaking a beat before
//! the hotkey is fully down — the hand and the mouth do not wait for each
//! other — so a capture that starts at the keypress clips the first phoneme.
//! The recognizer does not report a clipped word; it reports a different word.
//! Keeping the microphone running and retaining the last few hundred
//! milliseconds costs one small ring buffer and removes an entire class of
//! "it misheard me" complaints.

use std::collections::VecDeque;
use weldspeak_protocol::audio::{FRAME_SAMPLES, PREROLL_FRAMES};

/// One 20 ms frame of `linear16` audio, ready for the wire.
pub type Frame = Vec<u8>;

/// Accumulates samples into frames, retaining a pre-roll while idle.
pub struct Framer {
    /// Samples not yet forming a whole frame.
    pending: Vec<i16>,
    /// Recent frames captured before the hotkey went down.
    preroll: VecDeque<Frame>,
    armed: bool,
}

impl Default for Framer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framer {
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(FRAME_SAMPLES * 2),
            preroll: VecDeque::with_capacity(PREROLL_FRAMES + 1),
            armed: false,
        }
    }

    /// Whether frames are currently being handed to the caller to send.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Frames currently held in the pre-roll.
    pub fn preroll_len(&self) -> usize {
        self.preroll.len()
    }

    /// Begin capturing, returning the retained pre-roll to send first.
    ///
    /// Called on hotkey-down. The returned frames are the audio from just
    /// *before* the press — the beginning of the word the user has already
    /// started saying.
    pub fn arm(&mut self) -> Vec<Frame> {
        self.armed = true;
        self.preroll.drain(..).collect()
    }

    /// Stop capturing and discard any partial frame.
    ///
    /// A partial frame is a fraction of 20 ms; sending it padded would append
    /// silence mid-utterance, and the recognizer has already had the audio that
    /// matters.
    pub fn disarm(&mut self) {
        self.armed = false;
        self.pending.clear();
        self.preroll.clear();
    }

    /// Feed samples; get back whole frames to send.
    ///
    /// While disarmed the frames go to the pre-roll instead and the result is
    /// empty — the microphone keeps running so there is something to pre-roll.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Frame> {
        self.pending.extend_from_slice(samples);

        let mut ready = Vec::new();

        while self.pending.len() >= FRAME_SAMPLES {
            let frame = encode(&self.pending[..FRAME_SAMPLES]);
            self.pending.drain(..FRAME_SAMPLES);

            if self.armed {
                ready.push(frame);
            } else {
                // Ring behaviour: the pre-roll holds the most recent audio and
                // nothing older, so idle time costs a fixed, tiny amount of memory.
                if self.preroll.len() == PREROLL_FRAMES {
                    self.preroll.pop_front();
                }
                self.preroll.push_back(frame);
            }
        }

        ready
    }
}

/// Encode samples as little-endian `linear16`.
///
/// Endianness is explicit rather than inherited from the host: the wire format
/// is fixed, and a big-endian build would otherwise send noise.
fn encode(samples: &[i16]) -> Frame {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use weldspeak_protocol::audio::FRAME_BYTES;

    fn ramp(count: usize) -> Vec<i16> {
        (0..count).map(|index| (index % 1000) as i16).collect()
    }

    #[test]
    fn emits_frames_of_exactly_twenty_milliseconds() {
        let mut framer = Framer::new();
        framer.arm();

        let frames = framer.push(&ramp(FRAME_SAMPLES * 3));

        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|frame| frame.len() == FRAME_BYTES));
    }

    #[test]
    fn holds_back_a_partial_frame_until_it_is_complete() {
        let mut framer = Framer::new();
        framer.arm();

        assert!(framer.push(&ramp(FRAME_SAMPLES - 1)).is_empty());
        // One more sample completes it.
        assert_eq!(framer.push(&ramp(1)).len(), 1);
    }

    #[test]
    fn reassembles_frames_across_arbitrary_chunk_sizes() {
        // Devices deliver whatever block size they like; frame boundaries must
        // not depend on it.
        let samples = ramp(FRAME_SAMPLES * 4);

        let mut framer = Framer::new();
        framer.arm();

        let mut frames = Vec::new();
        for chunk in samples.chunks(97) {
            frames.extend(framer.push(chunk));
        }

        assert_eq!(frames.len(), 4);

        let flattened: Vec<u8> = frames.concat();
        let expected: Vec<u8> = samples[..FRAME_SAMPLES * 4]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(flattened, expected);
    }

    #[test]
    fn encodes_little_endian_regardless_of_host() {
        let mut framer = Framer::new();
        framer.arm();

        let mut samples = vec![0i16; FRAME_SAMPLES];
        samples[0] = 0x0102;

        let frames = framer.push(&samples);
        assert_eq!(&frames[0][..2], &[0x02, 0x01]);
    }

    #[test]
    fn withholds_frames_while_idle() {
        let mut framer = Framer::new();

        assert!(framer.push(&ramp(FRAME_SAMPLES * 3)).is_empty());
        assert_eq!(framer.preroll_len(), 3);
    }

    #[test]
    fn hands_back_the_preroll_when_armed() {
        let mut framer = Framer::new();
        framer.push(&ramp(FRAME_SAMPLES * 3));

        let preroll = framer.arm();

        assert_eq!(preroll.len(), 3);
        assert_eq!(framer.preroll_len(), 0);
        assert!(framer.is_armed());
    }

    #[test]
    fn preroll_keeps_only_the_most_recent_audio() {
        let mut framer = Framer::new();

        // Far more idle audio than the pre-roll should retain.
        framer.push(&ramp(FRAME_SAMPLES * (PREROLL_FRAMES + 50)));

        assert_eq!(framer.preroll_len(), PREROLL_FRAMES);
    }

    #[test]
    fn preroll_holds_the_newest_frames_not_the_oldest() {
        let mut framer = Framer::new();

        // Distinguishable frames: each frame's samples carry its index.
        for index in 0..(PREROLL_FRAMES + 5) {
            framer.push(&vec![index as i16; FRAME_SAMPLES]);
        }

        let preroll = framer.arm();
        let first_value = i16::from_le_bytes([preroll[0][0], preroll[0][1]]);

        // The oldest five frames should have been dropped.
        assert_eq!(first_value, 5);
    }

    #[test]
    fn preroll_covers_the_intended_duration() {
        // The point of the buffer is 300 ms of lead-in; if the constants ever
        // drift apart, this is what notices.
        let mut framer = Framer::new();
        framer.push(&ramp(FRAME_SAMPLES * (PREROLL_FRAMES + 10)));

        let captured_ms = framer.preroll_len() * 20;
        assert_eq!(captured_ms, 300);
    }

    #[test]
    fn disarming_clears_everything() {
        let mut framer = Framer::new();
        framer.arm();
        framer.push(&ramp(FRAME_SAMPLES + 7));

        framer.disarm();

        assert!(!framer.is_armed());
        assert_eq!(framer.preroll_len(), 0);
        // The half-frame left over must not leak into the next utterance.
        assert!(framer.push(&ramp(FRAME_SAMPLES - 1)).is_empty());
        assert_eq!(framer.preroll_len(), 0);
    }

    #[test]
    fn a_second_dictation_starts_clean() {
        let mut framer = Framer::new();

        framer.arm();
        framer.push(&ramp(FRAME_SAMPLES));
        framer.disarm();

        framer.push(&ramp(FRAME_SAMPLES * 2));
        let preroll = framer.arm();

        assert_eq!(preroll.len(), 2);
    }
}
