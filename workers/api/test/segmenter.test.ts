/**
 * Cutting dictations at pauses.
 *
 * The audio is synthetic: "speech" is a loud tone, a "pause" is faint noise,
 * so where the cuts land can be checked to the frame.
 */

import { describe, expect, it } from "vitest";
import { frameDb, hasSpeech, Segmenter } from "../src/segmenter.js";
import { UploadClock } from "../src/session-do.js";

const RATE = 16_000;

/** `ms` of 16-bit PCM at roughly `db` dBFS. */
function tone(ms: number, db: number): Uint8Array {
  const samples = (RATE * ms) / 1000;
  const out = new Uint8Array(samples * 2);
  const view = new DataView(out.buffer);
  const amplitude = Math.pow(10, db / 20) * Math.SQRT2 * 32767;
  for (let i = 0; i < samples; i++) {
    view.setInt16(i * 2, Math.round(amplitude * Math.sin((2 * Math.PI * 220 * i) / RATE)), true);
  }
  return out;
}

const speech = (ms: number) => tone(ms, -20);
const pause = (ms: number) => tone(ms, -65);

function join(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.byteLength, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.byteLength;
  }
  return out;
}

/** Feed `audio` in client-sized 20 ms frames, or in `chunkBytes` pieces. */
function run(audio: Uint8Array, chunkBytes = 640): { pieces: Uint8Array[]; rest: Uint8Array } {
  const segmenter = new Segmenter();
  const pieces: Uint8Array[] = [];
  for (let offset = 0; offset < audio.byteLength; offset += chunkBytes) {
    pieces.push(...segmenter.push(audio.slice(offset, offset + chunkBytes)));
  }
  return { pieces, rest: segmenter.finish() };
}

const ms = (bytes: number) => (bytes / 2 / RATE) * 1000;

describe("frameDb", () => {
  it("measures loudness in dBFS", () => {
    expect(frameDb(tone(20, -20))).toBeCloseTo(-20, 0);
    expect(frameDb(new Uint8Array(640))).toBe(-120);
  });
});

describe("Segmenter", () => {
  it("leaves a short dictation whole", () => {
    const audio = join([speech(3_000), pause(600), speech(3_000)]);
    const { pieces, rest } = run(audio);
    expect(pieces).toHaveLength(0);
    expect(rest).toEqual(audio);
  });

  it("cuts inside a pause once a piece is long enough", () => {
    const audio = join([speech(10_000), pause(800), speech(4_000)]);
    const { pieces, rest } = run(audio);

    expect(pieces).toHaveLength(1);
    // In the pause, clear of both words.
    expect(ms(pieces[0]!.byteLength)).toBeGreaterThan(10_000);
    expect(ms(pieces[0]!.byteLength)).toBeLessThan(10_800);
    // Nothing lost or duplicated.
    expect(join([...pieces, rest])).toEqual(audio);
  });

  it("does not cut at the short gaps between words", () => {
    const words = Array.from({ length: 30 }, () => [speech(400), pause(150)]).flat();
    const { pieces } = run(join(words));
    expect(pieces).toHaveLength(0);
  });

  it("cuts a long run without pauses at its quietest moment", () => {
    const audio = join([speech(18_000), tone(40, -35), speech(6_000)]);
    const { pieces, rest } = run(audio);

    expect(pieces).toHaveLength(1);
    expect(ms(pieces[0]!.byteLength)).toBeGreaterThanOrEqual(18_000);
    expect(ms(pieces[0]!.byteLength)).toBeLessThanOrEqual(18_040);
    expect(join([...pieces, rest])).toEqual(audio);
  });

  it("cuts in the same place whatever size the chunks arrive in", () => {
    const audio = join([speech(9_000), pause(700), speech(9_000), pause(700), speech(2_000)]);
    const aligned = run(audio);
    const ragged = run(audio, 333);

    expect(ragged.pieces.map((p) => p.byteLength)).toEqual(aligned.pieces.map((p) => p.byteLength));
    expect(aligned.pieces).toHaveLength(2);
    expect(join([...ragged.pieces, ragged.rest])).toEqual(audio);
  });
});

describe("hasSpeech", () => {
  const floor = -70;

  it("finds nothing in the silence after the last word", () => {
    expect(hasSpeech(tone(700, -72), floor)).toBe(false);
    expect(hasSpeech(new Uint8Array(0), floor)).toBe(false);
  });

  it("hears a short word from a quiet microphone", () => {
    expect(hasSpeech(join([tone(300, -72), tone(250, -52), tone(300, -72)]), floor)).toBe(true);
  });

  it("ignores a click", () => {
    expect(hasSpeech(join([tone(300, -72), tone(40, -20), tone(300, -72)]), floor)).toBe(false);
  });
});

describe("UploadClock", () => {
  it("reads no lag when audio arrives in real time after the opening burst", () => {
    const clock = new UploadClock();
    clock.add(700, 0); // pre-roll and audio held while connecting
    for (let t = 20; t <= 5_000; t += 20) clock.add(20, t);
    expect(clock.lagAt(5_010)).toBeLessThanOrEqual(20);
  });

  it("reads the backlog when the link cannot keep up", () => {
    const clock = new UploadClock();
    clock.add(500, 0);
    // Four seconds of audio take eight seconds to arrive.
    for (let t = 40; t <= 8_000; t += 40) clock.add(20, t);
    expect(clock.lagAt(8_000)).toBeGreaterThanOrEqual(3_900);
  });
});
