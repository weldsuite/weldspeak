import { describe, expect, it } from "vitest";
import type { DictionaryTerm } from "@weldspeak/protocol";
import {
  glossaryKeyterms,
  rankTerms,
  recognitionLanguage,
  recognitionOptions,
} from "../src/session-do.js";

const term = (
  name: string,
  soundsLike: string | null = null,
  extra: Partial<DictionaryTerm> = {},
): DictionaryTerm => ({
  id: name,
  scope: "user",
  term: name,
  soundsLike,
  createdAt: "2026-01-01 00:00:00",
  ...extra,
});

describe("glossaryKeyterms", () => {
  it("boosts the written form, never the misheard one", () => {
    // Boosting "in colonel" pulled the recognizer towards the very mistake
    // the entry exists to fix.
    expect(glossaryKeyterms([term("Inconel 625", "in colonel six twenty five")])).toEqual([
      "Inconel 625",
    ]);
  });

  it("dedupes case-insensitively and skips blanks", () => {
    const out = glossaryKeyterms([term("TIG"), term("tig"), term("  "), term("MIG")]);
    expect(out.map((value) => value.toLowerCase()).sort()).toEqual(["mig", "tig"]);
  });

  it("keeps at most 50 terms, and not simply the first 50 alphabetically", () => {
    const filler = Array.from({ length: 80 }, (_, index) =>
      term(`Aaa${String(index).padStart(2, "0")}`),
    );
    const corrected = term("Zirconium", "sir cone ium");
    const out = glossaryKeyterms([...filler, corrected]);
    expect(out).toHaveLength(50);
    expect(out[0]).toBe("Zirconium");
  });

  it("stays inside Deepgram's token budget with long terms", () => {
    const long = Array.from({ length: 50 }, (_, index) => term(`${"x".repeat(100)}${index}`));
    const out = glossaryKeyterms(long);
    expect(out.join("").length).toBeLessThanOrEqual(1_200);
    expect(out.length).toBeGreaterThan(0);
  });
});

describe("rankTerms", () => {
  it("puts corrections first, then the org glossary, then newest personal terms", () => {
    const ranked = rankTerms([
      term("old personal", null, { createdAt: "2026-01-01 00:00:00" }),
      term("new personal", null, { createdAt: "2026-03-01 00:00:00" }),
      term("shared", null, { scope: "org" }),
      term("Claude Code", "cloud code"),
    ]).map((entry) => entry.term);
    expect(ranked).toEqual(["Claude Code", "shared", "new personal", "old personal"]);
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
