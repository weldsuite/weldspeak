/**
 * Transcript cleanup.
 *
 * The interesting cases are the failure modes. Cleanup output is injected
 * straight into whatever the user was typing into, so a model that returns a
 * preamble, a refusal, or an answer to the dictated question does visible
 * damage. Every one of those degrades to the raw transcript instead.
 */

import { describe, expect, it } from "vitest";
import type { DictionaryTerm } from "@weldspeak/protocol";
import {
  buildCleanupPrompt,
  cleanupTranscript,
  looksLikeAssistantReply,
  preservesDictation,
  stripModelChatter,
  CLEANUP_TIMEOUT_MS,
} from "../src/format.js";
import type { Env } from "../src/env.js";

/** An Env carrying only what cleanup touches, with a scripted model. */
function envWith(run: (model: string, input: unknown) => Promise<unknown>): Env {
  return {
    CLEANUP_MODEL: "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
    AI: { run: (model: string, input: unknown) => run(model, input) },
  } as unknown as Env;
}

const respondWith = (text: string) => envWith(async () => ({ response: text }));

const term = (name: string, soundsLike: string | null = null): DictionaryTerm => ({
  id: crypto.randomUUID(),
  scope: "org",
  term: name,
  soundsLike,
  createdAt: new Date().toISOString(),
});

describe("prompt construction", () => {
  it("wraps the transcript as data so a question is not treated as a chat turn", () => {
    const prompt = buildCleanupPrompt("what time is the meeting", []);

    expect(prompt).toContain("<dictation>");
    expect(prompt).toContain("what time is the meeting");
    expect(prompt).toMatch(/never an answer/i);
  });

  it("supplies glossary terms as spelling context", () => {
    const prompt = buildCleanupPrompt("we used inconel", [term("Inconel 625")]);

    expect(prompt).toContain("Inconel 625");
    expect(prompt).toContain("we used inconel");
    expect(prompt).toContain("<dictation>");
  });

  it("includes phonetic hints when a term has one", () => {
    const prompt = buildCleanupPrompt("x", [term("Inconel 625", "in-co-nel six twenty five")]);
    expect(prompt).toContain("sounds like: in-co-nel six twenty five");
  });
});

describe("stripping model chatter", () => {
  it("removes a leading preamble", () => {
    expect(stripModelChatter("Here is the cleaned text: The weld looks good.")).toBe(
      "The weld looks good.",
    );
  });

  it("removes code fences", () => {
    expect(stripModelChatter("```\nThe weld looks good.\n```")).toBe("The weld looks good.");
  });

  it("unwraps a fully quoted response", () => {
    expect(stripModelChatter('"The weld looks good."')).toBe("The weld looks good.");
  });

  it("leaves a quotation inside the text alone", () => {
    // The quotes here are content the user dictated, not a wrapper the model added.
    const dictated = 'He said "check the root pass" before leaving.';
    expect(stripModelChatter(dictated)).toBe(dictated);
  });

  it("leaves clean text untouched", () => {
    expect(stripModelChatter("The weld looks good.")).toBe("The weld looks good.");
  });
});

describe("reply detection", () => {
  it("flags a model that refuses to transcribe", () => {
    expect(
      looksLikeAssistantReply(
        "I'm not going to transcribe anything yet. Please go ahead and dictate the text you'd like me to clean up.",
      ),
    ).toBe(true);
  });

  it("keeps a dictated first-person sentence", () => {
    expect(looksLikeAssistantReply("I'm heading to site at three.")).toBe(false);
  });

  it("rejects an answer that does not keep the dictated words", () => {
    expect(preservesDictation("what is two plus two", "4")).toBe(false);
  });

  it("accepts a cleanup that keeps the utterance", () => {
    expect(preservesDictation("um the weld looks uh good", "The weld looks good.")).toBe(true);
  });
});

describe("cleanup", () => {
  it("returns the cleaned text when the model behaves", async () => {
    const env = respondWith("The weld looks good.");
    const result = await cleanupTranscript(env, "um the weld looks uh good", []);

    expect(result).toEqual({ text: "The weld looks good.", formatted: true });
  });

  it("passes the configured model and a low temperature", async () => {
    let seenModel = "";
    let seenInput: Record<string, unknown> = {};

    const env = envWith(async (model, input) => {
      seenModel = model;
      seenInput = input as Record<string, unknown>;
      return { response: "Hello." };
    });

    await cleanupTranscript(env, "hello", []);

    expect(seenModel).toBe("@cf/meta/llama-3.3-70b-instruct-fp8-fast");
    // Cleanup is a rewrite, not a creative task; near-greedy decoding stops the
    // model paraphrasing what it was told to preserve.
    expect(seenInput.temperature).toBeLessThanOrEqual(0.2);
    const messages = seenInput.messages as Array<{ content: string }>;
    expect(messages[1]?.content).toContain("<dictation>");
  });

  it("ships the raw transcript when the model exceeds its deadline", async () => {
    const env = envWith(
      () => new Promise((resolve) => setTimeout(() => resolve({ response: "too late" }), 5_000)),
    );

    const started = Date.now();
    const result = await cleanupTranscript(env, "raw words here", [], 50);
    const elapsed = Date.now() - started;

    expect(result).toEqual({ text: "raw words here", formatted: false });
    // The point of the deadline is that the user is not left waiting.
    expect(elapsed).toBeLessThan(1_000);
  });

  it("ships the raw transcript when the model throws", async () => {
    const env = envWith(async () => {
      throw new Error("inference failed");
    });

    expect(await cleanupTranscript(env, "raw words here", [])).toEqual({
      text: "raw words here",
      formatted: false,
    });
  });

  it("ships the raw transcript when the model returns nothing usable", async () => {
    for (const bad of ["", "   ", "```\n\n```"]) {
      const result = await cleanupTranscript(respondWith(bad), "raw words here", []);
      expect(result).toEqual({ text: "raw words here", formatted: false });
    }
  });

  it("rejects a response far longer than its input", async () => {
    // The classic failure: the model answers the dictation instead of cleaning
    // it. Injecting that into the user's document would be worse than useless.
    const essay = "Well, that depends on several factors. ".repeat(50);
    const result = await cleanupTranscript(respondWith(essay), "what do you think", []);

    expect(result).toEqual({ text: "what do you think", formatted: false });
  });

  it("ships the raw transcript when the model answers a dictated question", async () => {
    const result = await cleanupTranscript(
      respondWith("The meeting is at three o'clock."),
      "what time is the meeting",
      [],
    );

    expect(result).toEqual({ text: "what time is the meeting", formatted: false });
  });

  it("ships the raw transcript when the model asks for dictation instead of copying it", async () => {
    const result = await cleanupTranscript(
      respondWith(
        "I'm not going to transcribe anything yet. Please go ahead and dictate the text you'd like me to clean up.",
      ),
      "hello there",
      [],
    );

    expect(result).toEqual({ text: "hello there", formatted: false });
  });

  it("handles empty input without calling the model", async () => {
    let called = false;
    const env = envWith(async () => {
      called = true;
      return { response: "something" };
    });

    expect(await cleanupTranscript(env, "   ", [])).toEqual({ text: "", formatted: false });
    expect(called).toBe(false);
  });

  it("defaults to a deadline the cleanup model can actually meet", () => {
    expect(CLEANUP_TIMEOUT_MS).toBeGreaterThanOrEqual(2_000);
    expect(CLEANUP_TIMEOUT_MS).toBeLessThanOrEqual(4_000);
  });
});
