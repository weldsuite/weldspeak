/**
 * MAI-Transcribe through OpenRouter.
 *
 * The request shape is the contract: OpenRouter forwards the Azure options
 * as-is, so a wrong field name costs keyword biasing on every dictation.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import type { DictionaryTerm } from "@weldspeak/protocol";
import {
  biasPhrases,
  encodeWav,
  isWorkersAiModel,
  transcribe,
  transcriptionDeadlineMs,
  transcriptionLanguage,
  transcriptionRequest,
} from "../src/stt.js";

const term = (name: string, soundsLike: string | null = null): DictionaryTerm => ({
  id: name,
  scope: "user",
  term: name,
  soundsLike,
  createdAt: "2026-01-01T00:00:00.000Z",
});

const MODEL = "microsoft/mai-transcribe-2";
const env = { STT_MODEL: MODEL, OPENROUTER_API_KEY: "sk-or-test" };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("engine selection", () => {
  it("streams Workers AI models and batches everything else", () => {
    expect(isWorkersAiModel("@cf/deepgram/nova-3")).toBe(true);
    expect(isWorkersAiModel(MODEL)).toBe(false);
  });
});

describe("transcriptionLanguage", () => {
  it("forces English when no language is chosen", () => {
    expect(transcriptionLanguage(undefined)).toBe("en");
    expect(transcriptionLanguage("  ")).toBe("en");
  });

  it("sends the bare language code", () => {
    expect(transcriptionLanguage("en-US")).toBe("en");
    expect(transcriptionLanguage("nl")).toBe("nl");
    expect(transcriptionLanguage("pt_BR")).toBe("pt");
  });

  it("lets the model detect the language for multilingual speakers", () => {
    expect(transcriptionLanguage("multi")).toBeUndefined();
    expect(transcriptionLanguage("auto")).toBeUndefined();
  });
});

describe("biasPhrases", () => {
  it("biases toward written forms only, deduplicated", () => {
    expect(biasPhrases([term("WeldSuite", "weld suite"), term("weldsuite"), term("TIG")])).toEqual([
      "WeldSuite",
      "TIG",
    ]);
  });
});

describe("encodeWav", () => {
  it("wraps 16 kHz mono linear16 in a canonical header", () => {
    const pcm = new Uint8Array([1, 2, 3, 4]);
    const wav = encodeWav(pcm);
    const view = new DataView(wav.buffer);
    const ascii = (offset: number) => String.fromCharCode(...wav.slice(offset, offset + 4));

    expect(ascii(0)).toBe("RIFF");
    expect(ascii(8)).toBe("WAVE");
    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint16(34, true)).toBe(16);
    expect(view.getUint32(40, true)).toBe(4);
    expect(Array.from(wav.slice(44))).toEqual([1, 2, 3, 4]);
  });
});

describe("transcriptionRequest", () => {
  const wav = encodeWav(new Uint8Array(2));

  it("passes keyword biasing and the clean style under Azure's own names", () => {
    const body = transcriptionRequest(MODEL, wav, { locale: "en", terms: [term("WeldSuite")] });
    expect(body).toMatchObject({
      model: MODEL,
      language: "en",
      temperature: 0,
      input_audio: { format: "wav" },
      provider: {
        options: {
          azure: {
            phraseList: { phrases: ["WeldSuite"] },
            // Inside enhancedMode: one level up Azure ignores it and the fillers stay in.
            enhancedMode: { modelOptions: { transcribeStyle: "clean" } },
          },
        },
      },
    });
    expect(Buffer.from((body.input_audio as { data: string }).data, "base64").length).toBe(wav.length);
  });

  it("omits the language to auto-detect, and the options when asked", () => {
    const body = transcriptionRequest(MODEL, wav, { locale: "multi", terms: [], providerOptions: false });
    expect(body).not.toHaveProperty("language");
    expect(body).not.toHaveProperty("provider");
  });
});

describe("transcribe", () => {
  const pcm = new Uint8Array(3200);

  it("returns the recognized text", async () => {
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(Response.json({ text: " Ship WeldSuite today. " }));

    await expect(transcribe(env, pcm, { terms: [], audioMs: 100 })).resolves.toBe("Ship WeldSuite today.");
    const [url, init] = fetch.mock.calls[0]!;
    expect(url).toBe("https://openrouter.ai/api/v1/audio/transcriptions");
    expect((init?.headers as Record<string, string>).Authorization).toBe("Bearer sk-or-test");
  });

  it("retries without Azure options when the provider rejects them", async () => {
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response("unknown field phraseList", { status: 400 }))
      .mockResolvedValueOnce(Response.json({ text: "hello" }));

    await expect(transcribe(env, pcm, { terms: [term("WeldSuite")], audioMs: 100 })).resolves.toBe("hello");
    const retried = JSON.parse(fetch.mock.calls[1]![1]!.body as string);
    expect(retried).not.toHaveProperty("provider");
  });

  it("asks for the batch model when it is the fallback behind the streaming one", async () => {
    const fetch = vi.spyOn(globalThis, "fetch").mockResolvedValue(Response.json({ text: "hello" }));
    await transcribe({ ...env, STT_MODEL: `${MODEL}-streaming` }, pcm, { terms: [], audioMs: 100 });
    expect(JSON.parse(fetch.mock.calls[0]![1]!.body as string).model).toBe(MODEL);
  });

  it("throws with the status when OpenRouter fails", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("upstream down", { status: 502 }));
    await expect(transcribe(env, pcm, { terms: [], audioMs: 100 })).rejects.toMatchObject({ status: 502 });
  });

  it("throws without a key rather than calling out", async () => {
    const fetch = vi.spyOn(globalThis, "fetch");
    await expect(transcribe({ STT_MODEL: MODEL }, pcm, { terms: [], audioMs: 100 })).rejects.toThrow(
      /OPENROUTER_API_KEY/,
    );
    expect(fetch).not.toHaveBeenCalled();
  });

  it("allows longer recordings more time, within OpenRouter's limit", () => {
    expect(transcriptionDeadlineMs(5_000)).toBeLessThan(transcriptionDeadlineMs(60_000));
    expect(transcriptionDeadlineMs(5 * 60_000)).toBe(60_000);
  });
});

describe("rate limiting", () => {
  afterEach(() => vi.restoreAllMocks());

  it("waits out a 429 once instead of giving the piece to the fallback", async () => {
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response("slow down", { status: 429, headers: { "Retry-After": "0.25" } }))
      .mockResolvedValueOnce(Response.json({ text: "Thanks, Dana." }));

    await expect(transcribe(env, new Uint8Array(3200), { terms: [], audioMs: 100 })).resolves.toBe("Thanks, Dana.");
    expect(fetch).toHaveBeenCalledTimes(2);
  });
});
