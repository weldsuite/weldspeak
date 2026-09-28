/**
 * Cut a dictation into pieces at pauses, while the speaker is still talking.
 *
 * MAI-Transcribe only takes whole recordings, so transcribing everything on
 * release made the wait grow with the length of the dictation. Cutting at a
 * pause and sending each finished piece straight away means that on release
 * only the last piece is left: a two-minute prompt waits about as long as a
 * sentence.
 *
 * Cuts land in the middle of a pause, never inside a word. A piece is at
 * least `minSegmentMs` long, because each cut costs the recognizer the
 * context across it; without any pause, the quietest moment of the last few
 * seconds is used once a piece reaches `maxSegmentMs`.
 */

import { SAMPLE_RATE } from "@weldspeak/protocol";

/** Analysis frame: 20 ms of 16-bit mono PCM. */
const FRAME_MS = 20;
const FRAME_BYTES = (SAMPLE_RATE * 2 * FRAME_MS) / 1000;

/** Frames of history used to estimate the room's noise floor. */
const FLOOR_WINDOW = 250;
/** A frame this far above the noise floor is speech. */
const SPEECH_ABOVE_FLOOR_DB = 10;
/**
 * A pause is also this far below the recent speech level. Without it, a
 * stretch of steady talking lifts the floor estimate to the speech itself
 * and the speech starts to look like silence.
 */
const PAUSE_BELOW_SPEECH_DB = 15;

export interface SegmenterOptions {
  /** Shortest piece worth sending on its own. */
  minSegmentMs: number;
  /** Silence that counts as a pause between phrases, not a gap between words. */
  pauseMs: number;
  /** Longest piece before one is cut at the quietest recent moment. */
  maxSegmentMs: number;
}

/**
 * Short pieces at clause-length pauses keep the last piece small, and the
 * last piece is all that is left to wait for on release. MAI-Transcribe
 * returned identical text for a 32-second prompt whole and in pieces.
 */
export const DEFAULT_SEGMENTER: SegmenterOptions = {
  minSegmentMs: 4_000,
  pauseMs: 300,
  maxSegmentMs: 20_000,
};

/** Loudness of one frame of 16-bit little-endian PCM, in dBFS. */
export function frameDb(frame: Uint8Array): number {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  let sum = 0;
  const samples = frame.byteLength >> 1;
  for (let i = 0; i < samples; i++) {
    const s = view.getInt16(i * 2, true) / 32768;
    sum += s * s;
  }
  const rms = Math.sqrt(sum / Math.max(1, samples));
  return rms > 0 ? 20 * Math.log10(rms) : -120;
}

/**
 * Whether a piece holds any speech, judged against the dictation's noise
 * floor rather than a fixed level, so a quiet microphone still counts. The
 * trailing silence after the last word would otherwise cost a full
 * recognizer round trip, and recognizers given silence tend to invent a word.
 */
export function hasSpeech(pcm: Uint8Array, floorDb: number): boolean {
  let loudMs = 0;
  for (let offset = 0; offset + FRAME_BYTES <= pcm.byteLength; offset += FRAME_BYTES) {
    if (frameDb(pcm.subarray(offset, offset + FRAME_BYTES)) > floorDb + SPEECH_ABOVE_FLOOR_DB) {
      loudMs += FRAME_MS;
    }
  }
  // A click is a frame or two; even "yes" is sustained for longer than this.
  return loudMs >= 120;
}

export class Segmenter {
  readonly #options: SegmenterOptions;
  /** Audio of the current piece, not yet sent. */
  #chunks: Uint8Array[] = [];
  /** Loudness of each whole frame in the current piece. */
  #frames: number[] = [];
  /** Bytes at the end of the piece that do not yet make a whole frame. */
  #partial = new Uint8Array(0);
  /** Recent frame loudness across pieces, for the noise floor. */
  #history: number[] = [];
  #floor = -120;
  #speech = -120;
  #silentRun = 0;

  /** The noise floor heard so far, in dBFS. */
  get floor(): number {
    return this.#floor;
  }

  constructor(options: SegmenterOptions = DEFAULT_SEGMENTER) {
    this.#options = options;
  }

  /** Add audio; returns the pieces that are finished and can be sent now. */
  push(chunk: Uint8Array): Uint8Array[] {
    this.#chunks.push(chunk);

    const ready: Uint8Array[] = [];
    const data = this.#partial.byteLength > 0 ? concat([this.#partial, chunk]) : chunk;
    let offset = 0;
    while (data.byteLength - offset >= FRAME_BYTES) {
      const cut = this.#analyse(frameDb(data.subarray(offset, offset + FRAME_BYTES)));
      offset += FRAME_BYTES;
      if (cut !== null) ready.push(this.#split(cut));
    }
    this.#partial = data.slice(offset);
    return ready;
  }

  /** Everything not yet sent, at the end of the dictation. */
  finish(): Uint8Array {
    const rest = concat(this.#chunks);
    this.#chunks = [];
    this.#frames = [];
    this.#partial = new Uint8Array(0);
    return rest;
  }

  /** Record a frame; returns the frame index to cut at, if a piece is done. */
  #analyse(db: number): number | null {
    this.#frames.push(db);
    this.#history.push(db);
    if (this.#history.length > FLOOR_WINDOW) this.#history.shift();
    if (this.#history.length % 25 === 0 || this.#history.length < 25) {
      const sorted = [...this.#history].sort((a, b) => a - b);
      this.#floor = sorted[Math.floor(sorted.length * 0.1)]!;
      this.#speech = sorted[Math.floor(sorted.length * 0.9)]!;
    }

    const silent =
      db < this.#floor + SPEECH_ABOVE_FLOOR_DB && db < this.#speech - PAUSE_BELOW_SPEECH_DB;
    this.#silentRun = silent ? this.#silentRun + 1 : 0;

    const pieceMs = this.#frames.length * FRAME_MS;
    const pauseFrames = this.#options.pauseMs / FRAME_MS;
    if (pieceMs >= this.#options.minSegmentMs && this.#silentRun >= pauseFrames) {
      // The middle of the pause: clear of the last word and of the next.
      return this.#frames.length - Math.ceil(this.#silentRun / 2);
    }
    if (pieceMs >= this.#options.maxSegmentMs) {
      // No pause long enough: the quietest moment of the last three seconds.
      const from = Math.max(1, this.#frames.length - 150);
      let quietest = from;
      for (let i = from; i < this.#frames.length; i++) {
        if (this.#frames[i]! < this.#frames[quietest]!) quietest = i;
      }
      return quietest;
    }
    return null;
  }

  /** Send frames [0, frame) as a piece; keep the rest as the next one. */
  #split(frame: number): Uint8Array {
    const all = concat(this.#chunks);
    // Whole frames analysed so far end before the partial tail.
    const cutByte = frame * FRAME_BYTES;
    const piece = all.slice(0, cutByte);
    this.#chunks = [all.slice(cutByte)];
    this.#frames = this.#frames.slice(frame);
    this.#silentRun = Math.min(this.#silentRun, this.#frames.length);
    return piece;
  }
}

function concat(chunks: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(chunks.reduce((total, chunk) => total + chunk.byteLength, 0));
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return out;
}
