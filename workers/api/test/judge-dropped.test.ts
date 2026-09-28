/**
 * Cleanups that drop part of what was said must be rejected, so the raw
 * transcript ships instead. The first case passed before word occurrences
 * were counted: its closing sentence reused words said earlier.
 */

import { expect, it } from "vitest";
import { judgeCleanup } from "../src/cleanup-rules.js";

const cases: Array<[string, string, string]> = [
  [
    "final sentence reuses earlier words",
    "okay so for the release update the changelog with the new transcription engine then bump the desktop version and build the installers for windows and mac and post a note in the team channel and after that send the changelog to the team",
    "Okay, so for the release, update the changelog with the new transcription engine. Then bump the desktop version and build the installers for Windows and Mac, and post a note in the team channel.",
  ],
  [
    "short final sentence in a long prompt",
    "i want you to refactor the auth middleware so that it checks the device token first and only falls back to the clerk session if there is no device token and make sure the error messages stay the same because the desktop app matches on them and add tests for the fallback path thanks",
    "I want you to refactor the auth middleware so that it checks the device token first and only falls back to the Clerk session if there is no device token. Make sure the error messages stay the same, because the desktop app matches on them.",
  ],
  [
    "middle sentence dropped",
    "please send the updated weld procedure to the whole team before friday and ask marco to check the inconel samples in the lab and then book the meeting room for monday morning",
    "Please send the updated weld procedure to the whole team before Friday, and then book the meeting room for Monday morning.",
  ],
];

for (const [name, raw, cleaned] of cases) {
  it(name, () => {
    const verdict = judgeCleanup(raw, cleaned);
    expect(verdict.ok).toBe(false);
  });
}
