import { describe, expect, it } from "vitest";
import type { DictionaryTerm } from "@weldspeak/protocol";
import { glossaryKeyterms } from "../src/session-do.js";

const term = (
  name: string,
  soundsLike: string | null = null,
): DictionaryTerm => ({
  id: name,
  scope: "user",
  term: name,
  soundsLike,
  createdAt: "2026-01-01T00:00:00.000Z",
});

describe("glossaryKeyterms", () => {
  it("includes soundsLike spellings as extra boosts", () => {
    expect(glossaryKeyterms([term("Inconel 625", "in-co-nel")])).toEqual([
      "Inconel 625",
      "in-co-nel",
    ]);
  });

  it("dedupes case-insensitively and skips empty soundsLike", () => {
    expect(
      glossaryKeyterms([
        term("TIG", null),
        term("tig", "tee eye gee"),
        term("MIG", "  "),
      ]),
    ).toEqual(["TIG", "tee eye gee", "MIG"]);
  });
});
