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
  appStyle,
  buildCleanupPrompt,
  cleanupDeadlineMs,
  cleanupTranscript,
  endsMidSentence,
  fitToCursor,
  judgeCleanup,
  looksLikeAssistantReply,
  stripModelChatter,
  CLEANUP_TIMEOUT_MS,
  MAX_CLEANUP_TIMEOUT_MS,
} from "../src/format.js";
import type { Env } from "../src/env.js";

/** An Env carrying only what cleanup touches, with a scripted model. */
function envWith(run: (model: string, input: unknown) => Promise<unknown>): Env {
  return {
    CLEANUP_MODEL: "@cf/google/gemma-4-26b-a4b-it",
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
    const prompt = buildCleanupPrompt("what time is the meeting", { terms: [] });

    expect(prompt).toContain("<transcript>");
    expect(prompt).toContain("what time is the meeting");
    expect(prompt).toMatch(/never an answer/i);
  });

  it("supplies glossary terms as spelling context", () => {
    const prompt = buildCleanupPrompt("we used inconel", { terms: [term("Inconel 625")] });

    expect(prompt).toContain("Inconel 625");
    expect(prompt).toContain("we used inconel");
    expect(prompt).toContain("<transcript>");
  });

  it("includes phonetic hints when a term has one", () => {
    const prompt = buildCleanupPrompt("x", { terms: [term("Inconel 625", "in-co-nel six twenty five")] });
    expect(prompt).toContain("sounds like: in-co-nel six twenty five");
  });

  it("adds a style hint for the destination app", () => {
    expect(buildCleanupPrompt("x", { terms: [], appName: "Cursor" })).toMatch(/prompt for an AI/);
    expect(buildCleanupPrompt("x", { terms: [], appName: "Outlook" })).toMatch(/email/);
    // Unknown apps and browsers get no hint rather than a wrong one.
    expect(buildCleanupPrompt("x", { terms: [], appName: "Firefox" })).not.toMatch(/Destination/);
  });

  it("buckets apps by name", () => {
    expect(appStyle("Code")).toBe("code");
    expect(appStyle("Claude")).toBe("code");
    expect(appStyle("WindowsTerminal")).toBe("code");
    expect(appStyle("idea64")).toBe("code");
    expect(appStyle("Visual Studio Code")).toBe("code");
    expect(appStyle("olk")).toBe("email");
    expect(appStyle("ms-teams")).toBe("chat");
    expect(appStyle("Slack")).toBe("chat");
    expect(appStyle("Mail")).toBe("email");
    expect(appStyle("chrome")).toBe("default");
    expect(appStyle(null)).toBe("default");
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

  it("drops a thinking trace that has only a closing tag", () => {
    // GLM-5.3-flash does this despite enable_thinking: false.
    expect(
      stripModelChatter('The transcript: "um the weld looks uh good"\nRemove filler.</think>The weld looks good.'),
    ).toBe("The weld looks good.");
  });

  it("drops a thinking trace if the model ignored enable_thinking: false", () => {
    expect(stripModelChatter("<think>fix punctuation</think>\nThe weld looks good.")).toBe(
      "The weld looks good.",
    );
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
    expect(judgeCleanup("what is the capital of portugal", "Lisbon.").ok).toBe(false);
  });

  it("accepts a cleanup that keeps the utterance", () => {
    expect(judgeCleanup("um the weld looks uh good", "The weld looks good.").ok).toBe(true);
  });

  it("accepts a self-correction that drops the abandoned words", () => {
    expect(judgeCleanup("send it to john wait to sarah", "Send it to Sarah.").ok).toBe(true);
    expect(
      judgeCleanup(
        "let's meet on thursday no actually wednesday after lunch",
        "Let's meet on Wednesday after lunch.",
      ).ok,
    ).toBe(true);
  });

  it("accepts misheard-word fixes and dictated symbols", () => {
    expect(
      judgeCleanup("open index dot ts and rename user underscore id", "Open index.ts and rename user_id.").ok,
    ).toBe(true);
  });
});

describe("rejecting shortened cleanups", () => {
  const prompt =
    "refactor the auth middleware so it checks the device token first and only falls back to the clerk session when there is no device token keep the error messages identical because the desktop app matches on them leave the refresh logic alone and add tests for the fallback path";

  it("accepts a full cleanup of a long prompt", () => {
    const cleaned =
      "Refactor the auth middleware so it checks the device token first and only falls back to the Clerk session when there is no device token. Keep the error messages identical, because the desktop app matches on them. Leave the refresh logic alone, and add tests for the fallback path.";
    expect(judgeCleanup(prompt, cleaned)).toEqual({ ok: true });
  });

  it("rejects a cleanup that stops before the end", () => {
    // Losing only the last clause is a small share of the words, which the old
    // half-the-words check let through. The tail check catches it.
    const cutOff =
      "Refactor the auth middleware so it checks the device token first and only falls back to the Clerk session when there is no device token. Keep the error messages identical, because the desktop app matches on them. Leave the refresh logic alone.";
    expect(judgeCleanup(prompt, cutOff)).toEqual({ ok: false, reason: "cut_off" });
  });

  it("rejects a summary", () => {
    const summary = "Refactor auth middleware to prefer device tokens, keep errors, add tests.";
    expect(judgeCleanup(prompt, summary)).toEqual({ ok: false, reason: "dropped_words" });
  });
});

describe("cleanup", () => {
  it("returns the cleaned text when the model behaves", async () => {
    const env = respondWith("The weld looks good.");
    const result = await cleanupTranscript(env, "um the weld looks uh good", []);

    expect(result).toEqual({ text: "The weld looks good.", formatted: true });
  });

  it("reads the chat-completions response shape GLM returns", async () => {
    const env = envWith(async () => ({
      choices: [{ message: { content: "The weld looks good." } }],
    }));
    const result = await cleanupTranscript(env, "um the weld looks uh good", []);

    expect(result).toEqual({ text: "The weld looks good.", formatted: true });
  });

  it("passes the configured model, greedy decoding, and the app name", async () => {
    let seenModel = "";
    let seenInput: Record<string, unknown> = {};

    const env = envWith(async (model, input) => {
      seenModel = model;
      seenInput = input as Record<string, unknown>;
      return { response: "Hello." };
    });

    await cleanupTranscript(env, "hello", [], { appName: "Cursor" });

    expect(seenModel).toBe("@cf/google/gemma-4-26b-a4b-it");
    // Cleanup copies with corrections; greedy decoding keeps it on the speaker's words.
    expect(seenInput.temperature).toBe(0);
    expect(
      (seenInput.chat_template_kwargs as { enable_thinking?: boolean } | undefined)
        ?.enable_thinking,
    ).toBe(false);
    const messages = seenInput.messages as Array<{ content: string }>;
    expect(messages[1]?.content).toContain("<transcript>");
    expect(messages[1]?.content).toContain("Destination: Cursor");
  });

  it("ships the raw transcript when the model exceeds its deadline", async () => {
    const env = envWith(
      () => new Promise((resolve) => setTimeout(() => resolve({ response: "too late" }), 5_000)),
    );

    const started = Date.now();
    const result = await cleanupTranscript(env, "raw words here", [], { timeoutMs: 50 });
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

  it("ships the raw transcript when the model ran out of tokens", async () => {
    const env = envWith(async () => ({
      choices: [{ message: { content: "The weld looks" }, finish_reason: "length" }],
    }));
    const result = await cleanupTranscript(env, "um the weld looks uh good", []);

    expect(result).toEqual({ text: "um the weld looks uh good", formatted: false });
  });

  it("ships the raw transcript when the model cut a long prompt short", async () => {
    const raw =
      "please update the readme with the new install steps then bump the version in package json and finally tag the release on github";
    const result = await cleanupTranscript(
      respondWith("Please update the README with the new install steps, then bump the version in package.json."),
      raw,
      [],
    );

    expect(result).toEqual({ text: raw, formatted: false });
  });

  it("defaults to a deadline the cleanup model can actually meet", () => {
    expect(CLEANUP_TIMEOUT_MS).toBeGreaterThanOrEqual(2_000);
    expect(CLEANUP_TIMEOUT_MS).toBeLessThanOrEqual(4_000);
  });

  it("gives long dictations more time, up to a ceiling", () => {
    expect(cleanupDeadlineMs("short")).toBeLessThan(CLEANUP_TIMEOUT_MS + 100);
    // A long AI prompt (~1,300 characters) took ~4 s on Llama 4 Scout; a fixed
    // 2.5 s deadline shipped every one of them raw.
    expect(cleanupDeadlineMs("x".repeat(1_300))).toBeGreaterThan(5_000);
    expect(cleanupDeadlineMs("x".repeat(100_000))).toBe(MAX_CLEANUP_TIMEOUT_MS);
  });
});

describe("cursor context", () => {
  const midSentence = { before: "Thanks for the update, I think we should", after: "" };

  it("tells the model to continue a sentence the cursor is inside", () => {
    const prompt = buildCleanupPrompt("move the launch to friday", {
      terms: [],
      field: midSentence,
    });
    expect(prompt).toContain("<before_cursor>");
    expect(prompt).toContain("I think we should");
    expect(prompt).toMatch(/start with a lowercase letter/);
    expect(prompt).toMatch(/read-only context/);
  });

  it("does not ask for lowercase after a finished sentence", () => {
    const prompt = buildCleanupPrompt("next item", {
      terms: [],
      field: { before: "That is done.\n" },
    });
    expect(prompt).not.toMatch(/lowercase/);
  });

  it("detects where a sentence stops", () => {
    expect(endsMidSentence("I think we should")).toBe(true);
    expect(endsMidSentence("Hi Aysha,")).toBe(true);
    expect(endsMidSentence("That is done.")).toBe(false);
    expect(endsMidSentence('He said "yes."')).toBe(false);
    expect(endsMidSentence("Line one\n")).toBe(false);
    expect(endsMidSentence(undefined)).toBe(false);
  });

  it("uses the window title to tell a Gmail tab from a Slack one", () => {
    expect(appStyle("chrome", "Inbox (3) - Gmail - Google Chrome")).toBe("email");
    expect(appStyle("msedge", "general | Slack - Microsoft Edge")).toBe("chat");
    expect(appStyle("firefox", "ChatGPT — Mozilla Firefox")).toBe("code");
    // A specific app wins over whatever its title says.
    expect(appStyle("Code", "Mail merge.ts - Visual Studio Code")).toBe("code");
    // Free text in a document title is not a site.
    expect(appStyle("WINWORD", "Mail merge.docx - Word")).toBe("default");
    const prompt = buildCleanupPrompt("x", {
      terms: [],
      appName: "chrome",
      field: { windowTitle: "Inbox - Gmail" },
    });
    expect(prompt).toContain('window "Inbox - Gmail"');
    expect(prompt).toMatch(/writing an email/);
  });

  it("sends the model only the text nearest the cursor", () => {
    const before = `${"old paragraph ".repeat(200)}the nearest words`;
    const prompt = buildCleanupPrompt("x", { terms: [], field: { before } });
    expect(prompt).toContain("the nearest words");
    expect(prompt.length).toBeLessThan(before.length);
  });

  it("rejects a cleanup that repeats the text before the cursor", () => {
    const field = { before: "Thanks for the update, I think we should" };
    expect(
      judgeCleanup(
        "move the launch to friday",
        "Thanks for the update, I think we should move the launch to Friday.",
        field,
      ),
    ).toEqual({ ok: false, reason: "echoed_context" });
    expect(judgeCleanup("move the launch to friday", "move the launch to Friday.", field)).toEqual({
      ok: true,
    });
  });

  it("allows words that were spoken even if they are also on screen", () => {
    const field = { before: "the root pass looks good to me" };
    expect(
      judgeCleanup("the root pass looks good to me too", "The root pass looks good to me too.", field)
        .ok,
    ).toBe(true);
  });

  it("passes the context to the model", async () => {
    let userMessage = "";
    const env = envWith(async (_model, input) => {
      userMessage = (input as { messages: Array<{ content: string }> }).messages[1]!.content;
      return { response: "move the launch to Friday" };
    });
    const result = await cleanupTranscript(env, "move the launch to friday", [], {
      field: midSentence,
    });
    expect(userMessage).toContain("I think we should");
    expect(result).toEqual({ text: "move the launch to Friday", formatted: true });
  });
});

describe("fitting text to the cursor", () => {
  it("adds a space after a preceding word", () => {
    expect(fitToCursor("move it to Friday.", { before: "I think we should" })).toBe(
      " move it to Friday.",
    );
  });

  it("adds no space at the start of a line or after an opening bracket", () => {
    expect(fitToCursor("Hello.", { before: "Notes:\n" })).toBe("Hello.");
    expect(fitToCursor("see below", { before: "(" })).toBe("see below");
    expect(fitToCursor(", and more", { before: "one" })).toBe(", and more");
  });

  it("adds a space before a following word", () => {
    expect(fitToCursor("really", { before: "It is ", after: "good" })).toBe("really ");
  });

  it("drops a period when punctuation already follows", () => {
    expect(fitToCursor("the new pricing.", { before: "We changed ", after: ", as agreed." })).toBe(
      "the new pricing",
    );
  });

  it("leaves text alone without context", () => {
    expect(fitToCursor("Hello.", undefined)).toBe("Hello.");
    expect(fitToCursor("Hello.", {})).toBe("Hello.");
  });
});
