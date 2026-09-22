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
//!
//! A second buffer mode covers the opposite race: the user speaks and releases
//! before the server says `ready`. Idle pre-roll is a 500 ms ring; once the
//! hotkey is down we *hold* every frame until streaming starts, so a slow
//! socket cannot age the utterance out of the ring.

use std::collections::VecDeque;
use weldspeak_protocol::audio::{FRAME_SAMPLES, PREROLL_FRAMES};

/// One 20 ms frame of `linear16` audio, ready for the wire.
pub type Frame = Vec<u8>;

/// Cap on audio retained between hotkey-down and `ready`.
///
/// Long enough for a slow Durable Object + upstream handshake; short enough
/// that a wedged session cannot grow without bound.
const HOLD_MAX_FRAMES: usize = 1_500; // 30 s at 20 ms/frame

/// Accumulates samples into frames, retaining a pre-roll while idle.
pub struct Framer {
    /// Samples not yet forming a whole frame.
    pending: Vec<i16>,
    /// Recent frames captured before the hotkey went down — or, while holding,
    /// every frame since the press.
    preroll: VecDeque<Frame>,
    armed: bool,
    /// Hotkey is down (or was): grow `preroll` without the idle ring eviction.
    holding: bool,
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
            holding: false,
        }
    }

    /// Whether frames are currently being handed to the caller to send.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Whether this utterance is retaining audio until streaming starts.
    pub fn is_holding(&self) -> bool {
        self.holding
    }

    /// Frames currently held in the pre-roll / hold buffer.
    pub fn preroll_len(&self) -> usize {
        self.preroll.len()
    }

    /// Start retaining every frame until [`Self::arm`].
    ///
    /// Called on hotkey-down. Keeps the existing idle pre-roll (lead-in before
    /// the press) and then appends speech spoken while the socket opens.
    pub fn hold(&mut self) {
        self.holding = true;
    }

    /// Begin streaming, returning every frame retained so far to send first.
    ///
    /// Called when the server is ready. The returned frames are the idle
    /// lead-in plus anything spoken while waiting — not only the last 300 ms.
    pub fn arm(&mut self) -> Vec<Frame> {
        self.armed = true;
        self.holding = false;
        self.preroll.drain(..).collect()
    }

    /// Stop capturing and discard any partial frame.
    ///
    /// A partial frame is a fraction of 20 ms; sending it padded would append
    /// silence mid-utterance, and the recognizer has already had the audio that
    /// matters.
    pub fn disarm(&mut self) {
        self.armed = false;
        self.holding = false;
        self.pending.clear();
        self.preroll.clear();
    }

    /// Feed samples; get back whole frames to send.
    ///
    /// While disarmed the frames go to the pre-roll / hold buffer instead and
    /// the result is empty — the microphone keeps running so there is something
    /// to flush when streaming starts.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Frame> {
        self.pending.extend_from_slice(samples);

        let mut ready = Vec::new();

        while self.pending.len() >= FRAME_SAMPLES {
            let frame = encode(&self.pending[..FRAME_SAMPLES]);
            self.pending.drain(..FRAME_SAMPLES);

            if self.armed {
                ready.push(frame);
            } else {
                let cap = if self.holding {
                    HOLD_MAX_FRAMES
                } else {
                    PREROLL_FRAMES
                };
                // Ring while idle; much larger ring while holding for `ready`.
                if self.preroll.len() == cap {
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
        // The point of the buffer is 500 ms of lead-in; if the constants ever
        // drift apart, this is what notices.
        let mut framer = Framer::new();
        framer.push(&ramp(FRAME_SAMPLES * (PREROLL_FRAMES + 10)));

        let captured_ms = framer.preroll_len() * 20;
        assert_eq!(captured_ms, 500);
    }

    #[test]
    fn hold_keeps_speech_while_waiting_for_ready() {
        // Idle ring would drop everything older than the pre-roll; holding must
        // not, or a quick tap before `ready` arrives ships silence.
        let mut framer = Framer::new();
        framer.push(&ramp(FRAME_SAMPLES * 3));
        framer.hold();

        framer.push(&ramp(FRAME_SAMPLES * (PREROLL_FRAMES + 20)));

        let held = framer.arm();
        assert_eq!(held.len(), 3 + PREROLL_FRAMES + 20);
        assert!(framer.is_armed());
        assert!(!framer.is_holding());
    }

    #[test]
    fn hold_still_caps_a_wedged_session() {
        let mut framer = Framer::new();
        framer.hold();
        framer.push(&ramp(FRAME_SAMPLES * (HOLD_MAX_FRAMES + 25)));

        assert_eq!(framer.preroll_len(), HOLD_MAX_FRAMES);
    }

    #[test]
    fn disarming_clears_everything() {
        let mut framer = Framer::new();
        framer.hold();
        framer.arm();
        framer.push(&ramp(FRAME_SAMPLES + 7));

        framer.disarm();

        assert!(!framer.is_armed());
        assert!(!framer.is_holding());
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
