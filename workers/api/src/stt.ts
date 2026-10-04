/**
 * Batch speech-to-text: MAI-Transcribe through OpenRouter.
 *
 * The Durable Object cuts the utterance at pauses and sends each piece here.
 * This is the whole recognizer when `STT_MODEL` names the batch model, and
 * the safety net behind the streaming model (see ./stt-stream.ts): when the
 * stream cannot be opened or breaks, the same audio is transcribed here.
 *
 * What it buys over streaming Nova-3 on Workers AI: 60 languages with
 * automatic detection and code switching, and keyword biasing that works in
 * every language rather than English only.
 *
 * Nova-3 stays reachable: an `STT_MODEL` that names a Workers AI model
 * (`@cf/...`) takes the streaming path in session-do.ts instead, so rolling
 * back is a config change rather than a revert.
 */

import { SAMPLE_RATE, type DictionaryTerm } from "@weldspeak/protocol";

const OPENROUTER_TRANSCRIPTIONS = "https://openrouter.ai/api/v1/audio/transcriptions";

/** Keyword-biasing budget. Hints, not forced output, so a long list only dilutes them. */
const MAX_PHRASES = 100;

/**
 * Time allowed for a transcription.
 *
 * MAI-Transcribe returns a sentence in well under a second; the allowance
 * grows with the audio so a five-minute dictation is not cut off. OpenRouter's
 * upstream gives up at 60 s, so waiting past that gains nothing.
 */
export function transcriptionDeadlineMs(audioMs: number): number {
  return Math.min(60_000, 8_000 + Math.ceil(audioMs / 2));
}

/** Whether `STT_MODEL` names a streaming Workers AI model rather than an OpenRouter one. */
export function isWorkersAiModel(model: string): boolean {
  return model.startsWith("@cf/");
}

/** Whether `STT_MODEL` names a model that streams through Vercel AI Gateway. */
export function isStreamingModel(model: string): boolean {
  return !isWorkersAiModel(model) && model.endsWith("-streaming");
}

/**
 * The batch model for `STT_MODEL`: itself, or the model a streaming one is
 * the live version of ("microsoft/mai-transcribe-2-streaming" falls back to
 * "microsoft/mai-transcribe-2").
 */
export function batchModel(model: string): string {
  return isStreamingModel(model) ? model.slice(0, -"-streaming".length) : model;
}

/**
 * The language to force, as an ISO-639-1 code, or undefined to auto-detect.
 *
 * English unless the speaker picked a language, as with Nova-3: forcing it is
 * what keeps English product names from being heard as Dutch. `multi` now
 * means what it says — MAI-Transcribe detects the language and follows code
 * switching — instead of Nova-3's word-by-word guess.
 */
export function transcriptionLanguage(locale: string | null | undefined): string | undefined {
  const chosen = locale?.trim().toLowerCase();
  if (!chosen) return "en";
  if (chosen === "multi" || chosen === "auto") return undefined;
  // "en-US" → "en"; OpenRouter takes the bare language code.
  return chosen.split(/[-_]/)[0];
}

/**
 * Dictionary terms to bias recognition toward.
 *
 * Only written forms: a `soundsLike` spelling is how a term is pronounced, and
 * biasing toward it would make the model write the pronunciation. Those hints
 * still reach the cleanup pass, which maps them onto the written form.
 */
export function biasPhrases(terms: DictionaryTerm[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const term of terms) {
    const value = term.term.trim();
    const key = value.toLowerCase();
    if (!value || seen.has(key)) continue;
    seen.add(key);
    out.push(value);
    if (out.length >= MAX_PHRASES) break;
  }
  return out;
}

/** Wrap 16 kHz mono linear16 PCM in a WAV header. */
export function encodeWav(pcm: Uint8Array, sampleRate = SAMPLE_RATE): Uint8Array {
  const header = new ArrayBuffer(44);
  const view = new DataView(header);
  const ascii = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i));
  };
  const channels = 1;
  const bytesPerSample = 2;

  ascii(0, "RIFF");
  view.setUint32(4, 36 + pcm.byteLength, true);
  ascii(8, "WAVE");
  ascii(12, "fmt ");
  view.setUint32(16, 16, true); // fmt chunk size
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, channels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * channels * bytesPerSample, true);
  view.setUint16(32, channels * bytesPerSample, true);
  view.setUint16(34, bytesPerSample * 8, true);
  ascii(36, "data");
  view.setUint32(40, pcm.byteLength, true);

  const wav = new Uint8Array(44 + pcm.byteLength);
  wav.set(new Uint8Array(header), 0);
  wav.set(pcm, 44);
  return wav;
}

export interface TranscriptionOptions {
  locale?: string | null;
  terms: DictionaryTerm[];
  /** Include Azure's own options (keyword biasing, clean style). */
  providerOptions?: boolean;
}

/**
 * The OpenRouter request body.
 *
 * Keyword biasing and the transcription style are Azure's own parameters
 * (`phraseList`, `enhancedMode.modelOptions.transcribeStyle`), passed through
 * under `provider.options.azure` under Azure's names. "clean" drops fillers
 * and false starts at the source, leaving cleanup less to do. The style sits
 * inside `enhancedMode`, as in Azure's transcription definition: one level up
 * it is accepted and ignored, and the fillers stay in.
 */
export function transcriptionRequest(
  model: string,
  wav: Uint8Array,
  options: TranscriptionOptions,
): Record<string, unknown> {
  const language = transcriptionLanguage(options.locale);
  const phrases = biasPhrases(options.terms);
  const azure: Record<string, unknown> = { enhancedMode: { modelOptions: { transcribeStyle: "clean" } } };
  if (phrases.length > 0) azure.phraseList = { phrases };

  return {
    model,
    input_audio: { data: Buffer.from(wav).toString("base64"), format: "wav" },
    ...(language ? { language } : {}),
    temperature: 0,
    ...(options.providerOptions === false ? {} : { provider: { options: { azure } } }),
  };
}

export class TranscriptionError extends Error {
  /** HTTP status from OpenRouter, when it answered at all. */
  readonly status?: number;

  constructor(message: string, status?: number) {
    super(message);
    this.status = status;
  }
}

interface TranscriptionEnv {
  STT_MODEL: string;
  OPENROUTER_API_KEY?: string;
}

/**
 * Transcribe one utterance.
 *
 * A 400 with Azure options attached is retried once without them: OpenRouter
 * forwards those fields to Azure as-is, so a renamed option would otherwise
 * fail every dictation. Losing keyword biasing is better than losing the text.
 */
export async function transcribe(
  env: TranscriptionEnv,
  pcm: Uint8Array,
  options: TranscriptionOptions & { audioMs: number; signal?: AbortSignal },
): Promise<string> {
  const key = env.OPENROUTER_API_KEY;
  if (!key) throw new TranscriptionError("OPENROUTER_API_KEY is not set");

  const wav = encodeWav(pcm);
  const attempt = async (providerOptions: boolean) => {
    const deadline = AbortSignal.timeout(transcriptionDeadlineMs(options.audioMs));
    const signal = options.signal ? AbortSignal.any([options.signal, deadline]) : deadline;
    return fetch(OPENROUTER_TRANSCRIPTIONS, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${key}`,
        "Content-Type": "application/json",
        "X-Title": "WeldSpeak",
      },
      body: JSON.stringify(transcriptionRequest(batchModel(env.STT_MODEL), wav, { ...options, providerOptions })),
      signal,
    });
  };

  let response = await attempt(true);
  if (response.status === 429) {
    // Pieces of one dictation can be in flight together, and the provider
    // rate-limits bursts with a one-second Retry-After. Waiting that second
    // beats falling back to a weaker recognizer for the piece.
    const retryAfter = Number(response.headers.get("Retry-After") ?? "1");
    await response.body?.cancel();
    await new Promise((resolve) => setTimeout(resolve, Math.min(1_500, Math.max(250, retryAfter * 1000))));
    response = await attempt(true);
  }
  if (response.status === 400) {
    const detail = await errorDetail(response);
    console.warn(JSON.stringify({ msg: "transcription rejected provider options; retrying without", detail }));
    response = await attempt(false);
  }
  if (!response.ok) {
    throw new TranscriptionError(
      `Transcription failed (${response.status}): ${await errorDetail(response)}`,
      response.status,
    );
  }

  const body = (await response.json()) as { text?: unknown };
  if (typeof body.text !== "string") throw new TranscriptionError("Transcription response had no text");
  return body.text.trim();
}

/**
 * Recognizer used when MAI-Transcribe fails.
 *
 * MAI-Transcribe reaches us through OpenRouter and an Azure preview, two hops
 * that can each go down. Batch Nova-3 on Workers AI shares neither, and the
 * whole utterance is already in memory, so an outage costs accuracy instead of
 * the dictation.
 */
export const FALLBACK_STT_MODEL = "@cf/deepgram/nova-3";

/** Transcribe the same audio with batch Nova-3. English options as on the streaming path. */
export async function transcribeFallback(
  ai: Ai,
  pcm: Uint8Array,
  locale: string | null | undefined,
): Promise<string> {
  const language = transcriptionLanguage(locale) ?? "multi";
  const english = language === "en";
  const response = (await ai.run(FALLBACK_STT_MODEL, {
    audio: { body: new Response(encodeWav(pcm)).body!, contentType: "audio/wav" },
    language,
    punctuate: true,
    smart_format: true,
    ...(english ? { dictation: true, numerals: true } : {}),
  } as never)) as { results?: { channels?: Array<{ alternatives?: Array<{ transcript?: string }> }> } };

  const text = response?.results?.channels?.[0]?.alternatives?.[0]?.transcript;
  if (typeof text !== "string") throw new TranscriptionError("Fallback transcription returned no text");
  return text.trim();
}

/** The start of an error body, which names the rejected option when there is one. */
async function errorDetail(response: Response): Promise<string> {
  try {
    return (await response.text()).slice(0, 300);
  } catch {
    return "(no body)";
  }
}
