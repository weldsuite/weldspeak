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
  // The next four passed while the ending was only checked by its content
  // words: small words and numbers are not content words, and a short
  // dictation may lose one content word anyway.
  [
    "short dictation loses an ending of small words",
    "can you check if this is working for me",
    "Can you check if this is working?",
  ],
  [
    "counting stops early",
    "testing one two three four five six",
    "Testing one two three.",
  ],
  [
    "counting stops early, written as digits",
    "testing one two three four five six",
    "Testing 1, 2, 3.",
  ],
  [
    "closing clause with one content word",
    "update the pricing page so the pro plan shows the yearly discount and move the faq below the plans and then we are done",
    "Update the pricing page so the Pro plan shows the yearly discount, and move the FAQ below the plans.",
  ],
];

for (const [name, raw, cleaned] of cases) {
  it(name, () => {
    const verdict = judgeCleanup(raw, cleaned);
    expect(verdict.ok).toBe(false);
  });
}

// An ending the cleanup is there to change is not a cut.
const kept: Array<[string, string, string]> = [
  ["trailing filler", "the weld looks good so yeah", "The weld looks good."],
  ["spoken punctuation at the end", "is the weld good question mark", "Is the weld good?"],
  ["line break at the end", "thanks for the report enter", "Thanks for the report."],
  ["misheard last word fixed", "deploy it to versel", "Deploy it to Vercel."],
  ["number written as a digit at the end", "the meeting is at five", "The meeting is at 5."],
  ["time written out at the end", "the meeting is at three thirty", "The meeting is at 3:30."],
  ["stuttered last word", "send it to the team team", "Send it to the team."],
];

for (const [name, raw, cleaned] of kept) {
  it(`keeps a cleanup with a ${name}`, () => {
    expect(judgeCleanup(raw, cleaned)).toEqual({ ok: true });
  });
}
