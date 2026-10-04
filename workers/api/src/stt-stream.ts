/**
 * Streaming speech-to-text: MAI-Transcribe-2-Streaming through Vercel AI Gateway.
 *
 * The audio goes to the recognizer while it is being spoken, so releasing the
 * hotkey leaves only the last words to wait for: a median 175 ms against
 * 850 ms for a batch request of the last piece (scripts/bench-stt.ts), and it
 * no longer grows with how long that piece is.
 *
 * What it gives up: the streaming model takes a language hint and nothing
 * else. Keyword biasing and the clean style belong to Azure's batch API, so a
 * dictionary term is put right after recognition (`protectTerms` and the
 * cleanup pass) rather than heard right in the first place.
 *
 * The stream is one more thing that can fail, and it never gets to cost a
 * dictation: the Durable Object keeps cutting the audio into pieces as it
 * arrives, and when the stream cannot be opened, breaks, or is late with its
 * final text, those pieces go to the batch recognizer in ./stt.ts.
 */

import { SAMPLE_RATE } from "@weldspeak/protocol";
import { TranscriptionError, transcriptionLanguage } from "./stt.js";

const GATEWAY_TRANSCRIPTION = "https://ai-gateway.vercel.sh/v4/ai/transcription-model";

/** The gateway's limit on one binary frame. */
const MAX_FRAME_BYTES = 65_536;

/** Time allowed for the handshake before the dictation goes to the batch recognizer. */
export const STREAM_CONNECT_TIMEOUT_MS = 4_000;

/**
 * Time allowed for the final text once the audio has ended.
 *
 * The stream answers in about 175 ms and took 1.1 s at worst in the
 * benchmark. Past this, transcribing the pieces in batch (about a second) is
 * the faster way out.
 */
export const STREAM_FINISH_TIMEOUT_MS = 3_000;

/** The gateway's URL for a streaming model, as `fetch` wants it for an upgrade. */
export function streamUrl(model: string): string {
  return `${GATEWAY_TRANSCRIPTION}?ai-model-id=${encodeURIComponent(model)}`;
}

/**
 * The subprotocols that open a transcription stream.
 *
 * The gateway reads its key from the handshake's subprotocol list, the one
 * place a browser socket can put it, and expects the marker protocol beside
 * it to have something to select.
 */
export function streamProtocols(key: string): string {
  return `ai-gateway-transcription.v1, ai-gateway-auth.${key}`;
}

/**
 * The first frame on the socket: the audio format, and the language to expect.
 *
 * The language follows the batch path (English unless chosen, omitted for
 * `multi` so the model detects it). Here it is a hint rather than a constraint.
 */
export function streamStartFrame(locale: string | null | undefined): Record<string, unknown> {
  const language = transcriptionLanguage(locale);
  return {
    type: "transcription-stream.start",
    inputAudioFormat: { type: "audio/pcm", rate: SAMPLE_RATE },
    ...(language ? { providerOptions: { azure: { language } } } : {}),
  };
}

/** A server frame, as far as this module reads it. */
interface StreamPart {
  type?: string;
  text?: string;
  delta?: string;
  warnings?: unknown[];
  error?: { message?: string };
}

/**
 * The text heard so far, for the overlay.
 *
 * Within one stretch of speech the gateway sends what is settled as deltas
 * and what is still open as a partial that replaces the last one; a final
 * closes the stretch with its whole text. A delta settles words the open
 * partial was already showing, and the partial without them only follows a
 * moment later, so they are taken off its front here rather than shown twice.
 */
export class StreamTranscript {
  #finals: string[] = [];
  #settled = "";
  #open = "";

  apply(part: StreamPart): void {
    if (part.type === "transcript-delta" && typeof part.delta === "string") {
      this.#settled += part.delta;
      if (this.#open.startsWith(part.delta)) this.#open = this.#open.slice(part.delta.length);
    } else if (part.type === "transcript-partial" && typeof part.text === "string") {
      this.#open = part.text;
    } else if (part.type === "transcript-final" && typeof part.text === "string") {
      this.#finals.push(part.text.trim());
      this.#settled = "";
      this.#open = "";
    }
  }

  get visible(): string {
    return [...this.#finals, `${this.#settled}${this.#open}`.trim()].filter(Boolean).join(" ");
  }
}

interface StreamEnv {
  STT_MODEL: string;
  AI_GATEWAY_API_KEY?: string;
}

export interface StreamHandlers {
  /** Everything heard so far, each time it changes. */
  onPartial(text: string): void;
  /** The stream is gone before `finish` was called; the caller takes over. */
  onFailure(reason: string): void;
}

/**
 * One dictation's stream.
 *
 * Connecting starts at construction and is not waited for: audio sent before
 * the socket is open is held and goes out behind the start frame, so the
 * handshake costs the speaker nothing.
 */
export class TranscriptionStream {
  readonly #handlers: StreamHandlers;
  readonly #transcript = new StreamTranscript();
  readonly #result: Promise<string>;
  #resolve!: (text: string) => void;
  #reject!: (error: TranscriptionError) => void;

  #socket: WebSocket | null = null;
  /** Audio waiting for the socket to open. */
  #held: Uint8Array[] = [];
  /** The text last passed to `onPartial`. */
  #reported = "";
  #finishing = false;
  #settled = false;
  readonly #connecting = new AbortController();

  constructor(env: StreamEnv, locale: string | null | undefined, handlers: StreamHandlers) {
    this.#handlers = handlers;
    this.#result = new Promise<string>((resolve, reject) => {
      this.#resolve = resolve;
      this.#reject = reject;
    });
    // A failure nobody is waiting on yet is reported through `onFailure`.
    this.#result.catch(() => {});
    void this.#connect(env, locale);
  }

  async #connect(env: StreamEnv, locale: string | null | undefined): Promise<void> {
    const key = env.AI_GATEWAY_API_KEY;
    if (!key) {
      this.#fail("AI_GATEWAY_API_KEY is not set");
      return;
    }

    // The signal stays tied to the socket the upgrade returns: aborting it
    // later drops the open stream. So the deadline is a timer cleared once
    // the handshake is over, not `AbortSignal.timeout`, which would cut
    // every dictation off four seconds in.
    let response: Response;
    const deadline = setTimeout(() => this.#connecting.abort(), STREAM_CONNECT_TIMEOUT_MS);
    try {
      response = await fetch(streamUrl(env.STT_MODEL), {
        headers: { Upgrade: "websocket", "Sec-WebSocket-Protocol": streamProtocols(key) },
        signal: this.#connecting.signal,
      });
    } catch (error) {
      this.#fail(`Could not reach the gateway: ${error}`);
      return;
    } finally {
      clearTimeout(deadline);
    }

    const socket = response.webSocket;
    if (!socket) {
      // A refused handshake is an ordinary response whose body says why: a
      // rate limit, a bad key, a model the team cannot use.
      let detail = "";
      try {
        detail = `: ${(await response.text()).slice(0, 300)}`;
      } catch {
        /* no readable body */
      }
      this.#fail(`Gateway refused the stream (${response.status})${detail}`, response.status);
      return;
    }

    socket.accept();
    if (this.#settled) {
      socket.close();
      return;
    }
    this.#socket = socket;
    socket.addEventListener("message", (event) => this.#onMessage(event.data));
    socket.addEventListener("close", () => this.#fail("Stream closed before the final text"));
    socket.addEventListener("error", () => this.#fail("Stream errored"));

    try {
      socket.send(JSON.stringify(streamStartFrame(locale)));
      for (const chunk of this.#held) this.#sendAudio(socket, chunk);
      this.#held = [];
      if (this.#finishing) socket.send(JSON.stringify({ type: "transcription-stream.audio-done" }));
    } catch (error) {
      this.#fail(`Lost the stream: ${error}`);
    }
  }

  #sendAudio(socket: WebSocket, chunk: Uint8Array): void {
    for (let offset = 0; offset < chunk.byteLength; offset += MAX_FRAME_BYTES) {
      socket.send(chunk.subarray(offset, offset + MAX_FRAME_BYTES));
    }
  }

  #onMessage(data: string | ArrayBuffer): void {
    if (typeof data !== "string" || this.#settled) return;

    let part: StreamPart;
    try {
      part = JSON.parse(data);
    } catch {
      return;
    }

    switch (part.type) {
      case "stream-start":
        // An option the model does not take comes back here instead of failing the stream.
        if (part.warnings?.length) {
          console.warn(JSON.stringify({ msg: "transcription stream warnings", warnings: part.warnings }));
        }
        break;
      case "transcript-delta":
      case "transcript-partial":
      case "transcript-final":
        this.#transcript.apply(part);
        this.#report();
        break;
      case "finish":
        this.#settled = true;
        this.#resolve((part.text ?? "").trim());
        this.close();
        break;
      case "error":
        this.#fail(part.error?.message ?? "Stream reported an error");
        break;
    }
  }

  /** Pass on the text heard so far, when it has changed and someone is still watching. */
  #report(): void {
    const text = this.#transcript.visible;
    if (this.#finishing || text === this.#reported) return;
    this.#reported = text;
    this.#handlers.onPartial(text);
  }

  /** Send audio, or hold it until the socket is open. */
  send(chunk: Uint8Array): void {
    if (this.#settled || this.#finishing) return;
    if (!this.#socket) {
      this.#held.push(chunk);
      return;
    }
    try {
      this.#sendAudio(this.#socket, chunk);
    } catch (error) {
      this.#fail(`Lost the stream: ${error}`);
    }
  }

  /**
   * End the audio and wait for the final text.
   *
   * Rejects when the stream fails or is not done within `timeoutMs`; the
   * caller then transcribes the recording in batch.
   */
  async finish(timeoutMs = STREAM_FINISH_TIMEOUT_MS): Promise<string> {
    if (!this.#finishing && !this.#settled) {
      this.#finishing = true;
      try {
        // Still connecting: the open handler sends this behind the held audio.
        this.#socket?.send(JSON.stringify({ type: "transcription-stream.audio-done" }));
      } catch (error) {
        this.#fail(`Lost the stream: ${error}`);
      }
    }
    this.#finishing = true;

    const timer = setTimeout(() => this.#fail("Stream did not finish in time"), timeoutMs);
    try {
      return await this.#result;
    } finally {
      clearTimeout(timer);
    }
  }

  /** Drop the stream. Safe to call at any point, any number of times. */
  close(): void {
    if (!this.#settled) {
      this.#settled = true;
      this.#reject(new TranscriptionError("Stream closed"));
    }
    this.#connecting.abort();
    this.#held = [];
    try {
      this.#socket?.close();
    } catch {
      /* already closed */
    }
    this.#socket = null;
  }

  #fail(reason: string, status?: number): void {
    if (this.#settled) return;
    this.#settled = true;
    this.#reject(new TranscriptionError(reason, status));
    const waiting = this.#finishing;
    this.close();
    // Once `finish` is waiting, the rejection tells the caller; before that, this does.
    if (!waiting) this.#handlers.onFailure(reason);
  }
}
