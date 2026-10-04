/**
 * One Durable Object per dictation.
 *
 * The DO owns the upstream connection to the speech model and sits between it
 * and the desktop client. That indirection buys three things a direct
 * client-to-model connection could not:
 *
 *   - Cloudflare credentials stay server-side; the client only ever holds a
 *     WeldSpeak token.
 *   - Audio can be buffered across an upstream reconnect, so a blip mid-
 *     utterance costs a few hundred milliseconds instead of the whole sentence.
 *   - Metering and quota are enforced where the audio actually flows, rather
 *     than trusting a client-reported duration.
 */

import { DurableObject } from "cloudflare:workers";
import {
  bytesToMs,
  parseClientFrame,
  SAMPLE_RATE,
  type ErrorCode,
  type ServerEvent,
  type StartFrame,
} from "@weldspeak/protocol";
import type { Env } from "./env.js";
import {
  countWords,
  FREE_MONTHLY_WORD_CAP,
  isWordQuotaExceeded,
  type Entitlement,
} from "./billing/entitlements.js";
import { cleanupNeeded, cleanupTranscript, fitToCursor, protectTerms } from "./format.js";
import { loadTerms } from "./routes/dictionary.js";
import { hasSpeech, Segmenter } from "./segmenter.js";
import { batchModel, isStreamingModel, isWorkersAiModel, transcribe, transcribeFallback } from "./stt.js";
import { TranscriptionStream } from "./stt-stream.js";

/** One transcribed piece of a dictation, and which recognizer produced it. */
interface Piece {
  text: string;
  engine: string;
}
import { loadOrgSettings, orgUsageSeconds, userWordCount } from "./routes/org.js";
import type { DictionaryTerm, FieldContext } from "@weldspeak/protocol";

/** Deepgram keyterm budget — keep the list short and unique. */
const MAX_KEYTERMS = 100;

/**
 * Build the Deepgram `keyterm` list from the glossary.
 *
 * Written forms and optional `soundsLike` spellings both boost recognition;
 * duplicates are dropped. Cap keeps the Workers AI payload bounded.
 */
export function glossaryKeyterms(terms: DictionaryTerm[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const term of terms) {
    for (const candidate of [term.term, term.soundsLike]) {
      const value = candidate?.trim();
      if (!value) continue;
      const key = value.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(value);
      if (out.length >= MAX_KEYTERMS) return out;
    }
  }
  return out;
}

/**
 * Recognition language for Deepgram.
 *
 * English unless the speaker picked a language. Streaming Nova-3 has no
 * language detection, and its `multi` code-switching mode, tried as the
 * default, decides the language word by word: English words that resemble
 * Dutch or German came out mangled, and product names were split. `multi`
 * stays available for people who mix languages, as an explicit choice.
 */
export function recognitionLanguage(locale: string | null | undefined): string {
  const chosen = locale?.trim();
  return chosen ? chosen : "en";
}

/**
 * Recognition options for the Nova-3 streaming call, used when `STT_MODEL`
 * names a Workers AI model rather than MAI-Transcribe (see ./stt.ts).
 *
 * Every value is a string. Workers AI validates this payload as all-strings
 * and rejects a number or boolean with a 400 ("expected a string"), so
 * `sample_rate: 16000` fails where `"16000"` succeeds. Only `keyterm` stays
 * structured, as an array of strings.
 *
 * Keyterms go out only for English. Nova-3 on Workers AI accepts `keyterm`
 * with any other language, `multi` included, then closes the stream at once
 * without transcribing anything: the client sees the speech model drop on
 * every dictation. The glossary still reaches the cleanup pass, which fixes
 * spellings for those languages.
 */
export function recognitionOptions(
  locale: string | null | undefined,
  keyterms: string[],
): Record<string, string | string[]> {
  const language = recognitionLanguage(locale);
  const english = language.startsWith("en");
  return {
    encoding: "linear16",
    sample_rate: String(SAMPLE_RATE),
    channels: "1",
    interim_results: "true",
    punctuate: "true",
    smart_format: "true",
    // Spoken "period" / "comma" → punctuation, and "twenty five" → "25".
    // Both are English-only in Deepgram; for other languages the cleanup
    // pass converts dictated punctuation instead.
    ...(english ? { dictation: "true", numerals: "true" } : {}),
    // Keep fillers out of the raw transcript; cleanup still strips hedges.
    filler_words: "false",
    // Client issues CloseStream on hotkey-up; do not auto-finalize on pause.
    endpointing: "false",
    language,
    ...(english && keyterms.length > 0 ? { keyterm: keyterms } : {}),
  };
}

/**
 * How far the client's upload fell behind real time.
 *
 * Audio is produced in real time, so on a link that keeps up, the audio
 * received stays a constant amount ahead of the wall clock: the pre-roll and
 * whatever was held while the session started, sent as a burst. On a link
 * that cannot carry 32 KB/s the lead shrinks, and the shortfall at `stop` is
 * how long the user waited for their own audio to arrive after letting go.
 */
export class UploadClock {
  #firstAt: number | null = null;
  #audioMs = 0;
  #maxLead = 0;

  /** Record `ms` of audio arriving at `now`. */
  add(ms: number, now: number): void {
    this.#firstAt ??= now;
    this.#audioMs += ms;
    this.#maxLead = Math.max(this.#maxLead, this.#audioMs - (now - this.#firstAt));
  }

  /** Milliseconds the upload was behind when `stop` arrived at `now`. */
  lagAt(now: number): number {
    if (this.#firstAt === null) return 0;
    const lead = this.#audioMs - (now - this.#firstAt);
    return Math.max(0, Math.round(this.#maxLead - lead));
  }
}

/** Identity handed to the DO by the Worker after it has authenticated the caller. */
export interface SessionIdentity {
  userId: string;
  orgId: string | null;
  /** Snapshot from the access token at connection time. */
  entitlement: Entitlement;
}

/**
 * Cap on a single utterance.
 *
 * Dictation is sentences and paragraphs, not lectures. A stuck hotkey or a
 * wedged client would otherwise stream audio — and bill for it — indefinitely.
 */
const MAX_UTTERANCE_MS = 5 * 60 * 1000;

export class DictationSession extends DurableObject<Env> {
  #client: WebSocket | null = null;
  #upstream: WebSocket | null = null;
  #identity: SessionIdentity | null = null;

  #started = false;
  #finalizing = false;
  #cancelled = false;

  #audioBytes = 0;
  #startFrame: StartFrame | null = null;
  #appName: string | null = null;

  /** Latest interim text, for the overlay. */
  #partial = "";
  /** Finalized segments, joined to form the transcript. */
  #finals: string[] = [];
  /** The recognizer has said it is finished: `Metadata` arrived or it closed. */
  #upstreamDone = false;
  /** When the recognizer last sent anything, for the post-stop quiet check. */
  #lastUpstreamAt = 0;

  /**
   * Batch recognition (MAI-Transcribe): audio is cut at pauses as it
   * arrives, and each finished piece is transcribed while the speaker is
   * still talking (see ./segmenter.ts). `stop` sends only the last piece.
   */
  #batch = false;
  #segmenter = new Segmenter();
  /** Transcriptions of the pieces sent so far, in order. */
  #pieces: Promise<Piece | null>[] = [];
  /**
   * Streaming recognition (MAI-Transcribe-2-Streaming, see ./stt-stream.ts):
   * the audio also goes to the recognizer as it arrives, and `stop` waits
   * only for its last words. The pieces are still cut but held back rather
   * than transcribed; they are what the batch recognizer gets if the stream
   * fails, after which the dictation carries on as a batch one.
   */
  #stream: TranscriptionStream | null = null;
  /** Pieces cut while the stream is live, in order. */
  #held: Uint8Array[] = [];
  /** Aborts an in-flight batch transcription when the dictation is cancelled. */
  #abort = new AbortController();
  /** Dictionary, loaded once at `start` for recognition and reused for cleanup. */
  #terms: DictionaryTerm[] = [];

  #upload = new UploadClock();
  /** Upload lag measured when `stop` arrived. */
  #uploadLagMs = 0;

  /** When each stage finished, for the timings sent with the result. */
  #marks: Partial<Record<"start" | "ready" | "stop" | "transcribed" | "cleaned", number>> = {};

  override async fetch(request: Request): Promise<Response> {
    const identityHeader = request.headers.get("X-WeldSpeak-Identity");
    if (!identityHeader) {
      return new Response("Missing identity", { status: 400 });
    }
    this.#identity = JSON.parse(identityHeader) as SessionIdentity;

    const pair = new WebSocketPair();
    const [clientSide, serverSide] = Object.values(pair) as [WebSocket, WebSocket];

    serverSide.accept();
    this.#client = serverSide;

    serverSide.addEventListener("message", (event) => {
      void this.#onClientMessage(event.data);
    });
    serverSide.addEventListener("close", () => this.#teardown());
    serverSide.addEventListener("error", () => this.#teardown());

    return new Response(null, { status: 101, webSocket: clientSide });
  }

  #send(event: ServerEvent): void {
    // The socket can close between an upstream event arriving and our
    // forwarding it; a failed send here is expected, not exceptional.
    try {
      this.#client?.send(JSON.stringify(event));
    } catch {
      /* client already gone */
    }
  }

  #fail(code: ErrorCode, message: string, retryable = false): void {
    this.#send({ type: "error", code, message, retryable });
    this.#teardown();
  }

  async #onClientMessage(data: string | ArrayBuffer): Promise<void> {
    if (data instanceof ArrayBuffer) {
      this.#onAudio(data);
      return;
    }

    let parsed: unknown;
    try {
      parsed = JSON.parse(data);
    } catch {
      this.#fail("bad_request", "Control frames must be JSON");
      return;
    }

    const frame = parseClientFrame(parsed);
    if (!frame) {
      this.#fail("bad_request", "Unrecognised control frame");
      return;
    }

    switch (frame.type) {
      case "start":
        await this.#onStart(frame);
        break;
      case "stop":
        await this.#onStop(frame.context);
        break;
      case "cancel":
        this.#cancelled = true;
        this.#teardown();
        break;
      case "ping":
        this.#send({ type: "pong" });
        break;
    }
  }

  async #onStart(frame: StartFrame): Promise<void> {
    if (this.#started) {
      this.#fail("bad_request", "Session already started");
      return;
    }
    if (frame.sampleRate !== SAMPLE_RATE) {
      this.#fail("bad_request", `Expected ${SAMPLE_RATE} Hz audio, got ${frame.sampleRate}`);
      return;
    }

    const identity = this.#identity!;
    this.#marks.start = Date.now();

    // Free-tier word cap is per person (UTC calendar month), across orgs.
    const wordsUsed = await userWordCount(this.env.DB, identity.userId);
    if (isWordQuotaExceeded(identity.entitlement, wordsUsed)) {
      this.#fail(
        "quota_exceeded",
        `Free plan is ${FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words per month. Subscribe at https://weldspeak.com/pricing`,
      );
      return;
    }

    // Optional org minute cap is an admin policy on top (paid orgs).
    const settings = await loadOrgSettings(this.env.DB, identity.orgId);
    if (settings?.monthlyMinuteCap != null) {
      const used = await orgUsageSeconds(this.env.DB, identity.orgId);
      if (used >= settings.monthlyMinuteCap * 60) {
        this.#fail("quota_exceeded", "This organisation has reached its monthly dictation limit");
        return;
      }
    }

    this.#started = true;
    this.#startFrame = frame;
    this.#appName = frame.appName ?? null;

    // The org glossary is merged server-side rather than trusting the client's
    // keyterm list: it keeps the vocabulary authoritative and stops a client
    // from probing another org's glossary by guessing terms.
    this.#terms = await loadTerms(this.env.DB, identity.userId, identity.orgId);
    this.#batch = !isWorkersAiModel(this.env.STT_MODEL);

    // Not waited for: audio is held until the socket opens, so the handshake
    // does not delay `ready`. Without a gateway key the dictation is a batch one.
    if (isStreamingModel(this.env.STT_MODEL) && this.env.AI_GATEWAY_API_KEY) {
      this.#stream = new TranscriptionStream(this.env, frame.locale, {
        onPartial: (text) => this.#send({ type: "partial", text }),
        onFailure: (reason) => this.#abandonStream(reason),
      });
    }

    if (!this.#batch) {
      // Include `soundsLike` hints as extra keyterms so pronunciation
      // spellings also boost the written form Deepgram should emit.
      try {
        await this.#connectUpstream(frame, glossaryKeyterms(this.#terms));
      } catch (error) {
        this.#fail("upstream_failed", `Could not reach the speech model: ${error}`, true);
        return;
      }
    }

    this.#marks.ready = Date.now();
    this.#send({ type: "ready", sessionId: crypto.randomUUID() });
  }

  /**
   * Open the streaming connection to the speech model.
   *
   * Workers AI returns a WebSocket for streaming models when called with
   * `{ websocket: true }`; recognition options travel in the same call, and
   * `recognitionOptions` explains them. They are fields declared on
   * Cloudflare's `@cf/deepgram/nova-3` input type — nothing invented.
   * Hold-to-talk closes the stream itself, so `endpointing` is disabled to
   * avoid mid-pause finals that discard acoustic context for the next phrase.
   */
  async #connectUpstream(frame: StartFrame, keyterms: string[]): Promise<void> {
    const response = (await this.env.AI.run(
      this.env.STT_MODEL as never,
      recognitionOptions(frame.locale, keyterms) as never,
      { websocket: true } as never,
    )) as unknown as Response & { webSocket?: WebSocket };

    const upstream = response?.webSocket;
    // A refused handshake comes back as an ordinary error response whose body
    // names the offending option. That body is the only thing worth having
    // when the model's schema drifts, so it travels with the error rather than
    // being swallowed into a bare "no WebSocket".
    if (!upstream) {
      let detail = "";
      try {
        detail = ` ${(await response.text()).slice(0, 300)}`;
      } catch {
        /* no readable body */
      }
      throw new Error(`Workers AI returned no WebSocket (status ${response?.status})${detail}`);
    }

    upstream.accept();
    this.#upstream = upstream;

    upstream.addEventListener("message", (event) => this.#onUpstreamMessage(event.data));
    upstream.addEventListener("close", () => {
      this.#upstreamDone = true;
      // Losing the upstream after `stop` is normal — it closes once it has sent
      // the final. Losing it mid-utterance is not.
      if (!this.#finalizing && !this.#cancelled && this.#started) {
        this.#fail("upstream_failed", "Speech model connection closed unexpectedly", true);
      }
    });
    upstream.addEventListener("error", () => {
      if (!this.#finalizing && !this.#cancelled) {
        this.#fail("upstream_failed", "Speech model connection errored", true);
      }
    });
  }

  /**
   * Parse a recognizer event.
   *
   * Deepgram nests the text at `channel.alternatives[0].transcript` and marks
   * finalized segments with `is_final`. Interim results replace the previous
   * partial; finals accumulate.
   */
  #onUpstreamMessage(data: string | ArrayBuffer): void {
    if (typeof data !== "string") return;

    let message: {
      type?: string;
      is_final?: boolean;
      channel?: { alternatives?: Array<{ transcript?: string }> };
    };
    try {
      message = JSON.parse(data);
    } catch {
      return;
    }

    this.#lastUpstreamAt = Date.now();
    // Deepgram's last word after CloseStream: every final has been sent.
    if (message.type === "Metadata") {
      if (this.#finalizing) this.#upstreamDone = true;
      return;
    }

    const transcript = message.channel?.alternatives?.[0]?.transcript;
    if (typeof transcript !== "string" || transcript.length === 0) return;

    if (message.is_final) {
      this.#finals.push(transcript);
      this.#partial = "";
    } else {
      this.#partial = transcript;
    }

    // Show finalized text plus whatever is still in flight, so the overlay
    // reads as one continuously growing sentence.
    const visible = [...this.#finals, this.#partial].filter(Boolean).join(" ");
    this.#send({ type: "partial", text: visible });
  }

  #onAudio(chunk: ArrayBuffer): void {
    if (!this.#started || this.#finalizing || this.#cancelled) return;

    this.#audioBytes += chunk.byteLength;
    this.#upload.add(bytesToMs(chunk.byteLength), Date.now());

    if (bytesToMs(this.#audioBytes) > MAX_UTTERANCE_MS) {
      this.#fail("bad_request", "Utterance exceeded the maximum length");
      return;
    }

    if (this.#batch) {
      const audio = new Uint8Array(chunk);
      for (const piece of this.#segmenter.push(audio)) {
        if (this.#stream) this.#held.push(piece);
        else this.#pieces.push(this.#transcribePiece(piece));
      }
      this.#stream?.send(audio);
      return;
    }

    try {
      this.#upstream?.send(chunk);
    } catch {
      this.#fail("upstream_failed", "Lost the speech model connection", true);
    }
  }

  async #onStop(field: FieldContext | undefined): Promise<void> {
    if (!this.#started || this.#finalizing) return;
    this.#finalizing = true;

    const identity = this.#identity!;
    const durationMs = bytesToMs(this.#audioBytes);
    this.#marks.stop = Date.now();
    this.#uploadLagMs = this.#upload.lagAt(this.#marks.stop);

    let raw: string;
    let engine: string = this.env.STT_MODEL;
    let pieces: number[] = [];
    if (this.#batch) {
      const recognized = (await this.#finishStream()) ?? (await this.#transcribeBatch());
      if (recognized === null) return;
      raw = recognized.text;
      engine = recognized.engine;
      pieces = recognized.pieceWords;
    } else {
      // Tell the recognizer no more audio is coming, then give it a moment to
      // flush its final segment. Without this the tail of the last word is lost.
      try {
        this.#upstream?.send(JSON.stringify({ type: "CloseStream" }));
      } catch {
        /* upstream already gone; whatever it sent is still in #finals */
      }
      await this.#awaitFinalTranscript();
      raw = [...this.#finals, this.#partial].filter(Boolean).join(" ").trim();
    }
    this.#marks.transcribed = Date.now();

    this.#send({ type: "transcript", text: raw });

    if (!raw) {
      this.#send({ type: "result", text: "", raw: "", formatted: false, durationMs, timings: this.#timings() });
      this.#teardown();
      return;
    }

    const shouldFormat = this.#startFrame?.format !== false;
    // Fast mode: paste the recognizer's text when cleanup has nothing to fix.
    const needs = shouldFormat && this.#startFrame?.fast ? cleanupNeeded(raw, this.#terms, field) : "off";
    const cleanup = !shouldFormat
      ? { text: raw, formatted: false, reason: "disabled" }
      : needs === null
        ? { text: protectTerms(raw, raw, this.#terms).text, formatted: false, reason: "fast" }
        : await cleanupTranscript(this.env, raw, this.#terms, { appName: this.#appName, field });
    const { text, formatted } = cleanup;
    this.#marks.cleaned = Date.now();

    // Spacing and punctuation at the cursor apply whether or not cleanup ran:
    // dictating mid-sentence should read like typing there would.
    const timings = this.#timings();
    this.#send({ type: "result", text: fitToCursor(text, field), raw, formatted, durationMs, timings });

    // One line per dictation, so a slow one can be pinned on a stage: the
    // recognizer, the cleanup model, or the setup before the first word.
    console.log(
      JSON.stringify({
        msg: "dictation timings",
        engine,
        audioMs: Math.round(durationMs),
        // Client → Worker as raw PCM, and Worker → OpenRouter as base64 WAV.
        uploadKB: Math.round(this.#audioBytes / 1024),
        // Words per piece, then before and after cleanup: counts only, never
        // text, so a dictation that lost words shows which stage lost them.
        ...(pieces.length > 0 ? { pieceWords: pieces } : {}),
        rawWords: countWords(raw),
        words: countWords(text),
        ...timings,
        formatted,
        ...(cleanup.reason ? { cleanup: cleanup.reason } : {}),
        // Why fast mode still ran cleanup, to tune what it skips.
        ...(needs && needs !== "off" ? { fastNeeds: needs } : {}),
      }),
    );

    // Persistence and metering happen after the result is on the wire: the
    // user has their text, and a slow write must not delay it.
    this.ctx.waitUntil(this.#persist(identity, raw, text, durationMs));

    this.#teardown();
  }

  /**
   * End the stream and take its text. Returns null when there is no stream or
   * it failed, in which case the pieces it was holding are already with the
   * batch recognizer and `#transcribeBatch` collects them.
   */
  async #finishStream(): Promise<{ text: string; engine: string; pieceWords: number[] } | null> {
    const stream = this.#stream;
    if (!stream) return null;
    try {
      return { text: await stream.finish(), engine: this.env.STT_MODEL, pieceWords: [] };
    } catch (error) {
      this.#abandonStream(String(error));
      return null;
    }
  }

  /** Give up on the stream and hand what it was holding to the batch recognizer. */
  #abandonStream(reason: string): void {
    const stream = this.#stream;
    if (!stream) return;
    this.#stream = null;
    stream.close();
    if (this.#cancelled) return;
    console.warn(JSON.stringify({ msg: "transcription stream failed; using batch", reason }));
    for (const piece of this.#held) this.#pieces.push(this.#transcribePiece(piece));
    this.#held = [];
  }

  /**
   * Send the last piece and collect every piece's text, in order. Returns
   * null when the session ended instead: cancelled, or a piece failed on
   * both recognizers and the client was told.
   */
  async #transcribeBatch(): Promise<{ text: string; engine: string; pieceWords: number[] } | null> {
    const floor = this.#segmenter.floor;
    const rest = this.#segmenter.finish();
    // Usually just the silence after the last word: nothing to send.
    if (hasSpeech(rest, floor)) this.#pieces.push(this.#transcribePiece(rest));
    const pieces = await Promise.all(this.#pieces);
    this.#pieces = [];
    if (this.#cancelled) return null;

    if (pieces.some((piece) => piece === null)) {
      this.#fail("upstream_failed", "Could not transcribe the recording", true);
      return null;
    }
    const done = pieces as Piece[];
    return {
      text: done.map((piece) => piece.text).filter(Boolean).join(" "),
      engine: done.some((piece) => piece.engine === "fallback") ? "fallback" : batchModel(this.env.STT_MODEL),
      pieceWords: done.map((piece) => countWords(piece.text)),
    };
  }

  /** Transcribe one piece with MAI-Transcribe, falling back to batch Nova-3. */
  async #transcribePiece(pcm: Uint8Array): Promise<Piece | null> {
    const locale = this.#startFrame?.locale;
    try {
      const text = await transcribe(this.env, pcm, {
        locale,
        terms: this.#terms,
        audioMs: bytesToMs(pcm.byteLength),
        signal: this.#abort.signal,
      });
      return { text, engine: batchModel(this.env.STT_MODEL) };
    } catch (error) {
      if (this.#cancelled) return null;
      console.warn(JSON.stringify({ msg: "transcription failed; using fallback", error: String(error) }));
    }

    try {
      return { text: await transcribeFallback(this.env.AI, pcm, locale), engine: "fallback" };
    } catch (error) {
      console.error(JSON.stringify({ msg: "fallback transcription failed", error: String(error) }));
      return null;
    }
  }

  /** Stage durations for this dictation, in milliseconds. */
  #timings(): Record<string, number> {
    const { start, ready, stop, transcribed, cleaned } = this.#marks;
    const span = (from?: number, to?: number) =>
      from !== undefined && to !== undefined ? Math.max(0, to - from) : undefined;
    const entries = {
      // Quota checks, dictionary load, and (streaming) the upstream handshake.
      setupMs: span(start, ready),
      // How late the client's audio arrived: waited out before `stop` could
      // even reach the server, so it is not part of serverMs.
      uploadLagMs: this.#uploadLagMs,
      // Batch: the whole recognition. Streaming: flushing the last words.
      sttMs: span(stop, transcribed),
      cleanupMs: span(transcribed, cleaned),
      // Release to result: the server's share of the wait the user feels.
      serverMs: span(stop, cleaned ?? transcribed),
    };
    return Object.fromEntries(
      Object.entries(entries).filter((entry): entry is [string, number] => entry[1] !== undefined),
    );
  }

  /**
   * Wait for the recognizer to finish after `CloseStream`.
   *
   * Done when it says so (`Metadata`, or the socket closes), or once a final
   * has arrived and the stream has then been quiet for `quietMs`. Returning on
   * the *first* final, as this used to, dropped the tail whenever a normal
   * final was already in flight when the user let go: that final satisfied the
   * wait and the flushed last words arrived after the result had gone.
   */
  async #awaitFinalTranscript(timeoutMs = 2_000, quietMs = 300): Promise<void> {
    const before = this.#finals.length;
    const deadline = Date.now() + timeoutMs;

    while (Date.now() < deadline) {
      if (this.#upstreamDone) return;
      if (this.#finals.length > before && Date.now() - this.#lastUpstreamAt >= quietMs) return;
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }

  async #persist(
    identity: SessionIdentity,
    raw: string,
    formatted: string,
    durationMs: number,
  ): Promise<void> {
    const day = new Date().toISOString().slice(0, 10);
    const audioSeconds = Math.round(durationMs / 1000);
    const wordCount = countWords(formatted);

    const statements = [
      this.env.DB.prepare(
        `INSERT INTO usage (clerk_org_id, clerk_user_id, day, audio_seconds, word_count)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (clerk_org_id, clerk_user_id, day)
         DO UPDATE SET
           audio_seconds = audio_seconds + excluded.audio_seconds,
           word_count = word_count + excluded.word_count`,
      ).bind(identity.orgId ?? "", identity.userId, day, audioSeconds, wordCount),
    ];

    // An admin who turned retention off means it: no server-side copy at all.
    // A person can also opt out for themselves ("Keep my dictations" off);
    // they can never opt back in over the org's choice.
    const settings = await loadOrgSettings(this.env.DB, identity.orgId);
    if (settings?.retainTranscripts !== false && this.#startFrame?.retain !== false) {
      statements.push(
        this.env.DB.prepare(
          `INSERT INTO transcripts
             (id, clerk_user_id, clerk_org_id, raw, formatted, duration_ms, app_name)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
        ).bind(
          crypto.randomUUID(),
          identity.userId,
          identity.orgId,
          raw,
          formatted,
          Math.round(durationMs),
          this.#appName,
        ),
      );
    }

    await this.env.DB.batch(statements);
  }

  #teardown(): void {
    this.#abort.abort();
    this.#segmenter.finish();
    this.#stream?.close();
    this.#stream = null;
    this.#held = [];
    try {
      this.#upstream?.close();
    } catch {
      /* already closed */
    }
    try {
      this.#client?.close();
    } catch {
      /* already closed */
    }
    this.#upstream = null;
    this.#client = null;
  }
}

