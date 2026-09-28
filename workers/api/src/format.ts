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

import type { DictionaryTerm, FieldContext } from "@weldspeak/protocol";
import type { Env } from "./env.js";
import {
  buildCleanupPrompt,
  cleanupMaxTokens,
  judgeCleanup,
  openRouterCleanupBody,
  protectTerms,
  stripModelChatter,
  SYSTEM_PROMPT,
} from "./cleanup-rules.js";
import { isWorkersAiModel } from "./stt.js";

export {
  appStyle,
  buildCleanupPrompt,
  cleanupNeeded,
  endsMidSentence,
  fitToCursor,
  judgeCleanup,
  looksLikeAssistantReply,
  protectTerms,
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
  /** Why the raw transcript shipped instead, for the timings log. */
  reason?: string;
}

export interface CleanupOptions {
  /** Focused application, used as a style hint (code, email, chat). */
  appName?: string | null;
  /** Text around the cursor and the window title, read at hotkey-down. */
  field?: FieldContext;
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
 * Cleanup model when `CLEANUP_MODEL` is on OpenRouter and OpenRouter fails.
 * The same model, so the text reads the same whichever one answered.
 */
export const FALLBACK_CLEANUP_MODEL = "@cf/google/gemma-4-26b-a4b-it";

type Messages = Array<{ role: string; content: string }>;

async function runOnWorkersAi(
  env: Env,
  model: string,
  messages: Messages,
  maxTokens: number,
): Promise<ModelOutput | null> {
  try {
    const response = await env.AI.run(model as never, {
      messages,
      // Cleanup is a copy with corrections, not creative writing: greedy
      // decoding keeps the model on the speaker's words.
      temperature: 0,
      max_tokens: maxTokens,
      // Reasoning models default to thinking. That would eat the deadline and
      // leak a trace into whatever the user was typing into.
      chat_template_kwargs: { enable_thinking: false },
    } as never);
    return extractCleanupText(response);
  } catch {
    return null;
  }
}

/**
 * The same request through OpenRouter.
 *
 * Measured with scripts/bench-cleanup.ts, Gemma 4 26B there answered in a
 * median 200 ms and 400 ms for a 200-word prompt; on Workers AI it took 860 ms
 * and 5.5 s, and missed the deadline on long prompts.
 */
async function runOnOpenRouter(
  env: Env,
  messages: Messages,
  maxTokens: number,
  signal: AbortSignal,
): Promise<ModelOutput | null> {
  if (!env.OPENROUTER_API_KEY) return null;
  try {
    const response = await fetch("https://openrouter.ai/api/v1/chat/completions", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${env.OPENROUTER_API_KEY}`,
        "Content-Type": "application/json",
        "X-Title": "WeldSpeak",
      },
      body: JSON.stringify(openRouterCleanupBody(env.CLEANUP_MODEL, messages, maxTokens)),
      signal,
    });
    if (!response.ok) {
      console.warn(
        JSON.stringify({ msg: "cleanup model failed", status: response.status, detail: (await response.text()).slice(0, 200) }),
      );
      return null;
    }
    return extractCleanupText(await response.json());
  } catch {
    return null;
  }
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
  const abort = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<"timeout">((resolve) => {
    timer = setTimeout(() => resolve("timeout"), timeoutMs);
  });

  const messages = [
    { role: "system", content: SYSTEM_PROMPT },
    {
      role: "user",
      content: buildCleanupPrompt(trimmed, {
        terms,
        appName: options.appName,
        field: options.field,
      }),
    },
  ];
  const maxTokens = cleanupMaxTokens(trimmed);

  const inference = (async (): Promise<ModelOutput | null> => {
    if (!isWorkersAiModel(env.CLEANUP_MODEL)) {
      const output = await runOnOpenRouter(env, messages, maxTokens, abort.signal);
      // A failed call leaves most of the deadline; the Workers AI copy of
      // the model can still use it.
      if (output || abort.signal.aborted) return output;
    }
    const model = isWorkersAiModel(env.CLEANUP_MODEL) ? env.CLEANUP_MODEL : FALLBACK_CLEANUP_MODEL;
    return runOnWorkersAi(env, model, messages, maxTokens);
  })();

  const output = await Promise.race([inference, timeout]);
  clearTimeout(timer);
  abort.abort();
  if (output === "timeout") return { text: trimmed, formatted: false, reason: "timeout" };
  if (output === null) return { text: trimmed, formatted: false, reason: "no_output" };
  if (output.truncated) return { text: trimmed, formatted: false, reason: "truncated" };

  const guarded = protectTerms(trimmed, stripModelChatter(output.text), terms);
  if (guarded.lost.length > 0) return { text: trimmed, formatted: false, reason: "lost_term" };

  const verdict = judgeCleanup(trimmed, guarded.text, options.field);
  if (!verdict.ok) return { text: trimmed, formatted: false, reason: verdict.reason };

  return { text: guarded.text, formatted: true };
}
