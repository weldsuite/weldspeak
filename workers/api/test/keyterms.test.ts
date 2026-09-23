import { describe, expect, it } from "vitest";
import type { DictionaryTerm } from "@weldspeak/protocol";
import { glossaryKeyterms, recognitionLanguage, recognitionOptions } from "../src/session-do.js";

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

describe("recognitionLanguage", () => {
  it("uses English when no language is chosen", () => {
    // Multilingual as the default mangled English dictation word by word.
    expect(recognitionLanguage(undefined)).toBe("en");
    expect(recognitionLanguage(null)).toBe("en");
    expect(recognitionLanguage("  ")).toBe("en");
  });

  it("keeps a language the user picked, multilingual included", () => {
    expect(recognitionLanguage("nl")).toBe("nl");
    expect(recognitionLanguage("en")).toBe("en");
    expect(recognitionLanguage("multi")).toBe("multi");
  });
});

describe("recognitionOptions", () => {
  const keyterms = ["WeldSuite", "WeldDesk"];

  it("boosts keyterms for English, the default", () => {
    expect(recognitionOptions(null, keyterms)).toMatchObject({ language: "en", keyterm: keyterms });
    expect(recognitionOptions("en", keyterms)).toMatchObject({ language: "en", keyterm: keyterms });
    expect(recognitionOptions("en-US", keyterms)).toMatchObject({ keyterm: keyterms });
  });

  it("never sends keyterms with other languages", () => {
    // Nova-3 on Workers AI closes the stream straight away when keyterms come
    // with a non-English language, so every dictation failed with the
    // speech model dropping.
    for (const locale of ["multi", "nl", "de"]) {
      expect(recognitionOptions(locale, keyterms)).not.toHaveProperty("keyterm");
    }
  });

  it("sends every scalar option as a string", () => {
    const options = recognitionOptions("en", keyterms);
    for (const [key, value] of Object.entries(options)) {
      if (key !== "keyterm") expect(typeof value).toBe("string");
    }
  });
});
