/**
 * Transcript cleanup.
 *
 * The recognizer returns what was said; this turns it into what the user meant
 * to type — fillers and false starts gone, self-corrections applied, STT slips
 * fixed, punctuation added. The prompt and the acceptance rules live in
 * ./cleanup-rules.ts; this file runs the model against a deadline.
 *
 * The model must never *answer* or *shorten* the dictation. People dictate
 * questions, instructions, and long AI prompts; injecting a reply or a
 * trimmed version is worse than leaving fillers in. Any output that fails the
 * rules ships the raw transcript instead.
 */

import type { DictionaryTerm } from "@weldspeak/protocol";
import type { Env } from "./env.js";
import {
  buildCleanupPrompt,
  cleanupMaxTokens,
  judgeCleanup,
  stripModelChatter,
  SYSTEM_PROMPT,
} from "./cleanup-rules.js";

export {
  appStyle,
  buildCleanupPrompt,
  judgeCleanup,
  looksLikeAssistantReply,
  stripModelChatter,
} from "./cleanup-rules.js";

/**
 * Deadline for a short dictation.
 *
 * A sentence or two finishes well inside this on Workers AI. Longer dictations
 * get more time from `cleanupDeadlineMs`: a fixed deadline meant every long
 * prompt missed it and silently shipped raw.
 */
export const CLEANUP_TIMEOUT_MS = 2_500;

/** Ceiling for the longest dictations; past it the user is left waiting. */
export const MAX_CLEANUP_TIMEOUT_MS = 10_000;

/** Generation time allowed per expected output token, beyond the base deadline. */
const MS_PER_OUTPUT_TOKEN = 12;

/** Deadline scaled to how much text the model has to write back. */
export function cleanupDeadlineMs(raw: string): number {
  const expectedTokens = Math.ceil(raw.length / 4);
  return Math.min(MAX_CLEANUP_TIMEOUT_MS, CLEANUP_TIMEOUT_MS + expectedTokens * MS_PER_OUTPUT_TOKEN);
}

export interface CleanupResult {
  text: string;
  /** False when cleanup was skipped, failed, or missed its deadline. */
  formatted: boolean;
}

export interface CleanupOptions {
  /** Focused application, used as a style hint (code, email, chat). */
  appName?: string | null;
  /** Override the length-scaled deadline; tests use a short one. */
  timeoutMs?: number;
}

interface ModelOutput {
  text: string;
  /** The model hit its token budget: whatever it wrote is cut off. */
  truncated: boolean;
}

/**
 * Pull the rewritten transcript out of either Workers AI response shape.
 *
 * Some Workers AI models return `{ response }`; others use the chat-completions
 * shape `{ choices: [{ message: { content }, finish_reason }] }`. Treating only
 * the first as success would make every cleanup miss and ship raw speech.
 */
function extractCleanupText(response: unknown): ModelOutput | null {
  if (!response || typeof response !== "object") return null;

  const record = response as {
    response?: unknown;
    finish_reason?: unknown;
    choices?: Array<{ message?: { content?: unknown }; finish_reason?: unknown }>;
  };

  if (typeof record.response === "string") {
    return { text: record.response, truncated: record.finish_reason === "length" };
  }

  const choice = record.choices?.[0];
  const content = choice?.message?.content;
  if (typeof content !== "string") return null;
  return { text: content, truncated: choice?.finish_reason === "length" };
}

/**
 * Run the cleanup pass, falling back to `raw` on timeout or failure.
 *
 * Never throws: every failure mode degrades to the raw transcript, because
 * losing a dictation is far worse than shipping an unpolished one.
 */
export async function cleanupTranscript(
  env: Env,
  raw: string,
  terms: DictionaryTerm[],
  options: CleanupOptions = {},
): Promise<CleanupResult> {
  const trimmed = raw.trim();
  if (!trimmed) return { text: "", formatted: false };

  const timeoutMs = options.timeoutMs ?? cleanupDeadlineMs(trimmed);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), timeoutMs);
  });

  const inference = (async (): Promise<ModelOutput | null> => {
    try {
      const response = (await env.AI.run(env.CLEANUP_MODEL as never, {
        messages: [
          { role: "system", content: SYSTEM_PROMPT },
          {
            role: "user",
            content: buildCleanupPrompt(trimmed, { terms, appName: options.appName }),
          },
        ],
        // Cleanup is a copy with corrections, not creative writing: greedy
        // decoding keeps the model on the speaker's words.
        temperature: 0,
        max_tokens: cleanupMaxTokens(trimmed),
        // Reasoning models default to thinking. That would eat the deadline and
        // leak a trace into whatever the user was typing into.
        chat_template_kwargs: { enable_thinking: false },
      } as never));

      return extractCleanupText(response);
    } catch {
      return null;
    }
  })();

  const output = await Promise.race([inference, timeout]);
  clearTimeout(timer);
  if (output === null || output.truncated) return { text: trimmed, formatted: false };

  const cleaned = stripModelChatter(output.text);
  if (!judgeCleanup(trimmed, cleaned).ok) {
    return { text: trimmed, formatted: false };
  }

  return { text: cleaned, formatted: true };
}
