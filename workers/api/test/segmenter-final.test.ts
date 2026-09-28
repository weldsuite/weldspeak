/**
 * The last piece of a dictation is skipped only when it is silence. Speech
 * with no pause in the final seconds must not read as silence, or the end of
 * the dictation would vanish.
 */

import { expect, it } from "vitest";
import { hasSpeech, Segmenter } from "../src/segmenter.js";

// Speech-like audio: syllables whose loudness wanders between -28 and -40 dBFS.
function syllables(ms: number): Uint8Array {
  const out = new Uint8Array((16 * ms) * 2);
  const view = new DataView(out.buffer);
  for (let i = 0; i < 16 * ms; i++) {
    const db = -34 + 6 * Math.sin((2 * Math.PI * i) / 3_200); // ~5 syllables/s
    const amp = Math.pow(10, db / 20) * Math.SQRT2 * 32767;
    view.setInt16(i * 2, Math.round(amp * Math.sin((2 * Math.PI * 180 * i) / 16_000)), true);
  }
  return out;
}
const quiet = (ms: number) => new Uint8Array(16 * ms * 2);

it("keeps a final piece of continuous speech", () => {
  const s = new Segmenter();
  const audio = [quiet(500), syllables(6_000), quiet(600), syllables(6_000)];
  const pieces: Uint8Array[] = [];
  for (const part of audio) for (let o = 0; o < part.byteLength; o += 640) pieces.push(...s.push(part.slice(o, o + 640)));
  const floor = s.floor;
  const rest = s.finish();
  expect(hasSpeech(rest, floor)).toBe(true);
});
