/**
 * Audio format constants shared by the desktop capture path, the Worker relay,
 * and the upstream speech model.
 *
 * These are not independently tunable. Deepgram's `linear16` encoding means
 * little-endian signed 16-bit PCM, and the desktop client resamples whatever
 * the microphone hands back to exactly this shape before framing. Changing a
 * value here requires changing the `start` frame sent upstream to match.
 */

/** Sample rate sent upstream, in Hz. */
export const SAMPLE_RATE = 16_000;

/** Wire encoding name understood by the upstream model. */
export const ENCODING = "linear16" as const;

/** Channel count. Dictation is single-speaker; mono halves the bytes on the wire. */
export const CHANNELS = 1;

/** Bytes per sample for `linear16`. */
export const BYTES_PER_SAMPLE = 2;

/**
 * Duration of one audio frame, in milliseconds.
 *
 * 20 ms is the usual voice-streaming quantum: small enough that the tail of an
 * utterance is not held back waiting for a buffer to fill, large enough that
 * per-message overhead stays negligible.
 */
export const FRAME_MS = 20;

/** Samples in one frame. */
export const FRAME_SAMPLES = (SAMPLE_RATE * FRAME_MS) / 1000; // 320

/** Bytes in one frame. */
export const FRAME_BYTES = FRAME_SAMPLES * BYTES_PER_SAMPLE * CHANNELS; // 640

/**
 * Pre-roll retained ahead of the hotkey press, in milliseconds.
 *
 * People start speaking fractionally before the key is fully down, so the
 * capture thread keeps a rolling buffer and prepends it to the stream. Without
 * this the first phoneme is clipped, which the model then guesses at — the
 * single cheapest accuracy win in the capture path.
 */
export const PREROLL_MS = 300;

/** Frames held in the pre-roll ring buffer. */
export const PREROLL_FRAMES = PREROLL_MS / FRAME_MS; // 15

/** Convert a byte count of `linear16` audio to its duration in milliseconds. */
export function bytesToMs(bytes: number): number {
  return (bytes / (SAMPLE_RATE * BYTES_PER_SAMPLE * CHANNELS)) * 1000;
}
