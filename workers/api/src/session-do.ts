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
import { cleanupTranscript } from "./format.js";
import { loadTerms } from "./routes/dictionary.js";
import { loadOrgSettings, orgUsageSeconds, userWordCount } from "./routes/org.js";
import type { DictionaryTerm } from "@weldspeak/protocol";

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
        await this.#onStop();
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
    // from probing another org's glossary by guessing terms. Include
    // `soundsLike` hints as extra keyterms so pronunciation spellings also
    // boost the written form Deepgram should emit.
    const terms = await loadTerms(this.env.DB, identity.userId, identity.orgId);
    const keyterms = glossaryKeyterms(terms);

    try {
      await this.#connectUpstream(frame, keyterms);
    } catch (error) {
      this.#fail("upstream_failed", `Could not reach the speech model: ${error}`, true);
      return;
    }

    this.#send({ type: "ready", sessionId: crypto.randomUUID() });
  }

  /**
   * Open the streaming connection to the speech model.
   *
   * Workers AI returns a WebSocket for streaming models when called with
   * `{ websocket: true }`; recognition options travel in the same call.
   *
   * Every option value is sent as a string. Workers AI validates this payload
   * as all-strings and rejects a number or boolean with a 400 ("expected a
   * string"), so `sample_rate: 16000` fails where `"16000"` succeeds. Only
   * `keyterm` stays structured, as an array of strings.
   *
   * Options below are fields declared on Cloudflare's
   * `@cf/deepgram/nova-3` input type — nothing invented. Hold-to-talk closes
   * the stream itself, so `endpointing` is disabled to avoid mid-pause
   * finals that discard acoustic context for the next phrase.
   */
  async #connectUpstream(frame: StartFrame, keyterms: string[]): Promise<void> {
    const response = (await this.env.AI.run(
      this.env.STT_MODEL as never,
      {
        encoding: "linear16",
        sample_rate: String(SAMPLE_RATE),
        channels: "1",
        interim_results: "true",
        punctuate: "true",
        smart_format: "true",
        // Spoken "period" / "comma" → punctuation (Deepgram dictation mode).
        dictation: "true",
        // Numerals: "twenty five" → "25" — useful for weld specs and sizes.
        numerals: "true",
        // Keep fillers out of the raw transcript; cleanup still strips hedges.
        filler_words: "false",
        // Client issues CloseStream on hotkey-up; do not auto-finalize on pause.
        endpointing: "false",
        ...(frame.locale ? { language: frame.locale } : {}),
        ...(keyterms.length > 0 ? { keyterm: keyterms } : {}),
      } as never,
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

    if (bytesToMs(this.#audioBytes) > MAX_UTTERANCE_MS) {
      this.#fail("bad_request", "Utterance exceeded the maximum length");
      return;
    }

    try {
      this.#upstream?.send(chunk);
    } catch {
      this.#fail("upstream_failed", "Lost the speech model connection", true);
    }
  }

  async #onStop(): Promise<void> {
    if (!this.#started || this.#finalizing) return;
    this.#finalizing = true;

    const identity = this.#identity!;
    const durationMs = bytesToMs(this.#audioBytes);

    // Tell the recognizer no more audio is coming, then give it a moment to
    // flush its final segment. Without this the tail of the last word is lost.
    try {
      this.#upstream?.send(JSON.stringify({ type: "CloseStream" }));
    } catch {
      /* upstream already gone; whatever it sent is still in #finals */
    }
    await this.#awaitFinalTranscript();

    const raw = [...this.#finals, this.#partial].filter(Boolean).join(" ").trim();

    this.#send({ type: "transcript", text: raw });

    if (!raw) {
      this.#send({ type: "result", text: "", raw: "", formatted: false, durationMs });
      this.#teardown();
      return;
    }

    const shouldFormat = this.#startFrame?.format !== false;
    const terms = shouldFormat
      ? await loadTerms(this.env.DB, identity.userId, identity.orgId)
      : [];

    const { text, formatted } = shouldFormat
      ? await cleanupTranscript(this.env, raw, terms)
      : { text: raw, formatted: false };

    this.#send({ type: "result", text, raw, formatted, durationMs });

    // Persistence and metering happen after the result is on the wire: the
    // user has their text, and a slow write must not delay it.
    this.ctx.waitUntil(this.#persist(identity, raw, text, durationMs));

    this.#teardown();
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
