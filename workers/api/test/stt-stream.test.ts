/**
 * MAI-Transcribe-2-Streaming through Vercel AI Gateway.
 *
 * The frames are the contract with the gateway, and the failure paths are the
 * contract with the Durable Object: a stream that cannot deliver has to say
 * so, exactly once, so the dictation can go to the batch recognizer instead.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  StreamTranscript,
  streamProtocols,
  streamStartFrame,
  streamUrl,
  TranscriptionStream,
} from "../src/stt-stream.js";
import { batchModel, isStreamingModel } from "../src/stt.js";

const MODEL = "microsoft/mai-transcribe-2-streaming";
const env = { STT_MODEL: MODEL, AI_GATEWAY_API_KEY: "vck_test" };

afterEach(() => {
  vi.restoreAllMocks();
});

async function until(condition: () => boolean, what: string): Promise<void> {
  for (let i = 0; i < 200; i++) {
    if (condition()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`Timed out waiting for ${what}`);
}

/** Stand in for the gateway: accept the upgrade and record what arrives. */
function fakeGateway() {
  const pair = new WebSocketPair();
  const [client, server] = Object.values(pair) as [WebSocket, WebSocket];
  server.accept();

  const frames: Array<Record<string, unknown> | number> = [];
  server.addEventListener("message", (event) => {
    frames.push(typeof event.data === "string" ? JSON.parse(event.data) : (event.data as ArrayBuffer).byteLength);
  });

  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(new Response(null, { status: 101, webSocket: client }));

  return {
    fetch,
    frames,
    server,
    say: (part: Record<string, unknown>) => server.send(JSON.stringify(part)),
  };
}

function handlers() {
  return { onPartial: vi.fn<(text: string) => void>(), onFailure: vi.fn<(reason: string) => void>() };
}

describe("engine selection", () => {
  it("streams the -streaming model and falls back to the model it is the live version of", () => {
    expect(isStreamingModel(MODEL)).toBe(true);
    expect(isStreamingModel("microsoft/mai-transcribe-2")).toBe(false);
    expect(isStreamingModel("@cf/deepgram/nova-3")).toBe(false);
    expect(batchModel(MODEL)).toBe("microsoft/mai-transcribe-2");
    expect(batchModel("microsoft/mai-transcribe-2")).toBe("microsoft/mai-transcribe-2");
  });
});

describe("handshake", () => {
  it("names the model in the URL and carries the key as a subprotocol", () => {
    expect(streamUrl(MODEL)).toBe(
      "https://ai-gateway.vercel.sh/v4/ai/transcription-model?ai-model-id=microsoft%2Fmai-transcribe-2-streaming",
    );
    expect(streamProtocols("vck_test")).toBe("ai-gateway-transcription.v1, ai-gateway-auth.vck_test");
  });

  it("describes the audio and hints the language, unless the model is to detect it", () => {
    expect(streamStartFrame("en-US")).toEqual({
      type: "transcription-stream.start",
      inputAudioFormat: { type: "audio/pcm", rate: 16_000 },
      providerOptions: { azure: { language: "en" } },
    });
    expect(streamStartFrame("multi")).not.toHaveProperty("providerOptions");
  });
});

describe("StreamTranscript", () => {
  it("joins what is settled with what is still open, across stretches of speech", () => {
    const transcript = new StreamTranscript();
    transcript.apply({ type: "transcript-partial", text: "What do" });
    expect(transcript.visible).toBe("What do");

    // The delta settles words the open partial still shows; they must not appear twice.
    transcript.apply({ type: "transcript-partial", text: "What do you" });
    transcript.apply({ type: "transcript-delta", delta: "What do" });
    expect(transcript.visible).toBe("What do you");
    transcript.apply({ type: "transcript-partial", text: " you think" });
    expect(transcript.visible).toBe("What do you think");

    transcript.apply({ type: "transcript-final", text: "What do you think?" });
    transcript.apply({ type: "transcript-partial", text: "I can" });
    expect(transcript.visible).toBe("What do you think? I can");
  });
});

describe("TranscriptionStream", () => {
  it("sends the audio held during the handshake behind the start frame, then the rest live", async () => {
    const gateway = fakeGateway();
    const stream = new TranscriptionStream(env, "en", handlers());
    // Before the socket is open.
    stream.send(new Uint8Array(640));
    stream.send(new Uint8Array(320));

    await until(() => gateway.frames.length === 3, "the held audio");
    stream.send(new Uint8Array(1280));
    const finished = stream.finish();
    await until(() => gateway.frames.length === 5, "audio-done");

    expect(gateway.frames).toEqual([
      streamStartFrame("en"),
      640,
      320,
      1280,
      { type: "transcription-stream.audio-done" },
    ]);
    const [url, init] = gateway.fetch.mock.calls[0]!;
    expect(url).toBe(streamUrl(MODEL));
    expect((init?.headers as Record<string, string>)["Sec-WebSocket-Protocol"]).toBe(streamProtocols("vck_test"));

    gateway.say({ type: "finish", text: " The weld looks good. ", segments: [] });
    await expect(finished).resolves.toBe("The weld looks good.");
  });

  it("ends the audio once connected when stop arrives during the handshake", async () => {
    const gateway = fakeGateway();
    const stream = new TranscriptionStream(env, "en", handlers());
    stream.send(new Uint8Array(640));
    const finished = stream.finish();

    await until(() => gateway.frames.length === 3, "audio-done");
    expect(gateway.frames[2]).toEqual({ type: "transcription-stream.audio-done" });

    gateway.say({ type: "finish", text: "Yes.", segments: [] });
    await expect(finished).resolves.toBe("Yes.");
  });

  it("reports the text as it grows", async () => {
    const gateway = fakeGateway();
    const events = handlers();
    new TranscriptionStream(env, "en", events);
    await until(() => gateway.frames.length === 1, "the start frame");

    gateway.say({ type: "stream-start", warnings: [] });
    gateway.say({ type: "transcript-partial", text: "The weld" });
    gateway.say({ type: "transcript-delta", delta: "The weld" });
    gateway.say({ type: "transcript-partial", text: " looks good." });

    // The delta changes nothing the speaker can see, so it is not reported.
    await until(() => events.onPartial.mock.calls.length === 2, "the partials");
    expect(events.onPartial.mock.calls.map(([text]) => text)).toEqual(["The weld", "The weld looks good."]);
  });

  it("hands over when the gateway refuses the stream", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("Rate limit exceeded", { status: 429 }));
    const events = handlers();
    new TranscriptionStream(env, "en", events);

    await until(() => events.onFailure.mock.calls.length === 1, "the failure");
    expect(events.onFailure.mock.calls[0]![0]).toMatch(/429.*Rate limit exceeded/);
  });

  it("hands over without calling out when there is no key", async () => {
    const fetch = vi.spyOn(globalThis, "fetch");
    const events = handlers();
    new TranscriptionStream({ STT_MODEL: MODEL }, "en", events);

    await until(() => events.onFailure.mock.calls.length === 1, "the failure");
    expect(events.onFailure.mock.calls[0]![0]).toMatch(/AI_GATEWAY_API_KEY/);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("hands over when the stream breaks mid-dictation", async () => {
    const gateway = fakeGateway();
    const events = handlers();
    const stream = new TranscriptionStream(env, "en", events);
    await until(() => gateway.frames.length === 1, "the start frame");

    gateway.say({ type: "error", error: { message: "invalid azure provider options" } });
    await until(() => events.onFailure.mock.calls.length === 1, "the failure");
    expect(events.onFailure).toHaveBeenCalledWith("invalid azure provider options");

    // Later audio is dropped rather than thrown on.
    stream.send(new Uint8Array(640));
    expect(events.onFailure).toHaveBeenCalledTimes(1);
  });

  it("rejects the wait, rather than reporting twice, when the stream dies after stop", async () => {
    const gateway = fakeGateway();
    const events = handlers();
    const stream = new TranscriptionStream(env, "en", events);
    await until(() => gateway.frames.length === 1, "the start frame");

    const finished = stream.finish();
    gateway.server.close(1011, "upstream gone");
    await expect(finished).rejects.toThrow(/closed before the final text/);
    expect(events.onFailure).not.toHaveBeenCalled();
  });

  it("gives up on a final text that does not come", async () => {
    const gateway = fakeGateway();
    const events = handlers();
    const stream = new TranscriptionStream(env, "en", events);
    await until(() => gateway.frames.length === 1, "the start frame");

    await expect(stream.finish(30)).rejects.toThrow(/did not finish in time/);
    expect(events.onFailure).not.toHaveBeenCalled();
  });
});
