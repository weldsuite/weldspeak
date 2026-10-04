/**
 * Benchmark MAI-Transcribe 2 through OpenRouter against Vercel AI Gateway.
 *
 * Both gateways serve the model from Azure, so the text should match and the
 * question is what each hop costs: latency per piece, how much of it is spent
 * at Azure, behaviour when the pieces of one dictation are in flight together,
 * and whether the Azure options production relies on (forced language, clean
 * style, keyword biasing) arrive intact.
 *
 * The clips are synthesized with MAI-Voice through OpenRouter and resampled
 * to the 16 kHz mono WAV the Durable Object sends, in the 2-20 s range the
 * segmenter cuts dictations into, plus one minute-long clip. They are cached,
 * so a rerun measures the same audio.
 *
 * Usage:
 *   pnpm --filter @weldspeak/api exec node --experimental-strip-types scripts/bench-stt.ts [case ...]
 *
 * OPENROUTER_API_KEY, AI_GATEWAY_API_KEY: required
 * BENCH_RUNS: runs per case (default 5)
 * BENCH_PACE_MS: least time between two requests to one gateway in the main
 *   pass (default 0). A gateway key limited to 5 requests a minute needs 13000.
 * BENCH_TRAIN: requests sent one after another, three trains (default 5)
 * BENCH_BURST: requests sent at once, three bursts (default 5)
 * BENCH_COOLDOWN_MS: quiet before each train and burst (default 0; 61000 for
 *   a rate-limited key)
 * BENCH_STREAM_RUNS: runs per case of MAI-Transcribe-2-Streaming, which only
 *   Vercel serves, fed at speaking pace (default 3)
 * BENCH_PHASES: which of cases,train,burst,stream to run (default all)
 * BENCH_AUDIO_DIR: clip cache (default: <tmp>/weldspeak-bench-stt)
 */

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const MODEL = "microsoft/mai-transcribe-2";
const TTS_MODEL = "microsoft/mai-voice-2.1-flash";
const TTS_RATE = 24_000;
/** Mirrors SAMPLE_RATE in @weldspeak/protocol. */
const SAMPLE_RATE = 16_000;

const RUNS = Number(process.env.BENCH_RUNS ?? 5);
const PACE_MS = Number(process.env.BENCH_PACE_MS ?? 0);
const TRAIN = Number(process.env.BENCH_TRAIN ?? 5);
const BURST = Number(process.env.BENCH_BURST ?? 5);
const COOLDOWN_MS = Number(process.env.BENCH_COOLDOWN_MS ?? 0);
const STREAM_RUNS = Number(process.env.BENCH_STREAM_RUNS ?? 3);
const PHASES = new Set((process.env.BENCH_PHASES ?? "cases,train,burst,stream").split(","));

const STREAM_MODEL = "microsoft/mai-transcribe-2-streaming";
/** Mirrors FRAME_BYTES in @weldspeak/protocol: 20 ms of audio, as the desktop sends it. */
const FRAME_BYTES = 640;
const FRAME_MS = 20;
const AUDIO_DIR = process.env.BENCH_AUDIO_DIR ?? join(tmpdir(), "weldspeak-bench-stt");

interface Case {
  id: string;
  /** What the voice reads, and what the transcript is scored against. */
  text: string;
  voice: string;
  /** Forced language, or undefined to auto-detect, as `multi` does in production. */
  language?: string;
  /** Dictionary terms sent as the phrase list. */
  terms?: string[];
  /** Case-insensitive substrings the transcript must (not) contain. */
  include?: string[];
  exclude?: string[];
  /** Skip word error rate: the clean style is expected to drop words. */
  unscored?: boolean;
}

const CASES: Case[] = [
  {
    id: "short-en",
    text: "The weld looks good.",
    voice: "en-US-Harper",
    language: "en",
  },
  {
    id: "sentence-en",
    text: "What do you think about the porosity on the second pass? I can grind it out before lunch.",
    voice: "en-US-Ethan",
    language: "en",
  },
  {
    id: "keyterms-en",
    text: "Aysha, can you send me the WeldSuite procedure for the Inconel 625 root pass before Thursday?",
    voice: "en-GB-Emily",
    language: "en",
    terms: ["Aysha Rahman", "WeldSuite", "Inconel 625"],
    include: ["WeldSuite", "Inconel 625"],
  },
  {
    id: "fillers-en",
    text: "Um, let's meet on Thursday, uh, after lunch, and, um, go through the numbers properly.",
    voice: "en-US-Harper",
    language: "en",
    include: ["Thursday", "after lunch", "numbers"],
    exclude: ["um,", "uh,", "um ", "uh "],
    unscored: true,
  },
  {
    id: "prompt-en",
    text:
      "I want you to refactor the auth middleware so that it checks the device token first and only falls back to the Clerk session if there is no device token. " +
      "Make sure the error messages stay the same, because the desktop app matches on them. " +
      "Don't touch the refresh logic at all, that's working fine, and add tests for the fallback path.",
    voice: "en-US-Jasper",
    language: "en",
    terms: ["Clerk"],
    include: ["device token", "Clerk", "refresh logic", "fallback path"],
  },
  {
    id: "dutch",
    text:
      "Ik wil dat je de inlogpagina ombouwt, zodat hij eerst het apparaattoken controleert en alleen terugvalt op de sessie als er geen token is. " +
      "Laat de foutmeldingen precies hetzelfde.",
    voice: "nl-NL-Sander",
    language: "nl",
    include: ["inlogpagina", "foutmeldingen"],
  },
  {
    id: "german",
    text: "Wir treffen uns am Mittwoch nach dem Mittagessen im Büro und besprechen die Schweißnähte am neuen Behälter.",
    voice: "de-DE-Mia",
    language: "de",
    include: ["Mittwoch", "Mittagessen", "Büro"],
  },
  {
    id: "switch-nl-en",
    text: "Kun je de pull request even reviewen voor de stand-up? De build is groen, maar de linter klaagt nog over unused imports.",
    voice: "nl-NL-Harper",
    include: ["pull request", "stand-up", "linter", "unused imports"],
  },
  {
    id: "minute-en",
    text:
      "Here's the context. We have a dictation app that runs on Windows and Mac, and the cleanup step keeps cutting off the end of long prompts. " +
      "First, figure out where the text is getting truncated. It could be the token limit, it could be the deadline, or it could be the validation that decides whether to ship the cleaned text or the raw transcript. " +
      "Second, I want a benchmark that runs the actual production prompt against a bunch of realistic dictations, including really long ones like this one, and reports for each model how often the output is accepted and how long it takes. " +
      "Third, once we know which model is best, switch the production config over to it, but keep the fallback behavior where, if anything goes wrong, we just ship the raw transcript, because losing someone's words is way worse than leaving in a few fillers. " +
      "One more thing: make sure the output still reads like me. I don't want it rewritten into corporate speak or summarized. I just want the punctuation fixed.",
    voice: "en-US-Olivia",
    language: "en",
    include: ["token limit", "deadline", "validation", "raw transcript", "corporate speak"],
  },
];

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name}`);
  return value;
}

const OPENROUTER_KEY = requireEnv("OPENROUTER_API_KEY");
const GATEWAY_KEY = requireEnv("AI_GATEWAY_API_KEY");

/** Mirrors encodeWav in src/stt.ts. */
function encodeWav(pcm: Buffer, sampleRate = SAMPLE_RATE): Buffer {
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + pcm.length, 4);
  header.write("WAVEfmt ", 8, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(sampleRate, 24);
  header.writeUInt32LE(sampleRate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(pcm.length, 40);
  return Buffer.concat([header, pcm]);
}

/**
 * Resample 16-bit mono PCM with a Hann-windowed sinc.
 *
 * The cutoff sits at the output's Nyquist frequency, so what the 24 kHz voice
 * holds above 8 kHz is removed rather than folded down into the speech band.
 */
function resample(pcm: Buffer, from: number, to: number): Buffer {
  const input = new Int16Array(pcm.buffer, pcm.byteOffset, pcm.length >> 1);
  const step = from / to;
  const cutoff = Math.min(1, to / from);
  const reach = 16;
  const out = Buffer.alloc(Math.floor(input.length / step) * 2);
  for (let n = 0; n < out.length >> 1; n++) {
    const center = n * step;
    const first = Math.ceil(center - reach);
    let sum = 0;
    for (let i = first; i <= center + reach; i++) {
      if (i < 0 || i >= input.length) continue;
      const x = i - center;
      const sinc = x === 0 ? 1 : Math.sin(Math.PI * cutoff * x) / (Math.PI * cutoff * x);
      const window = 0.5 + 0.5 * Math.cos((Math.PI * x) / reach);
      sum += input[i]! * cutoff * sinc * window;
    }
    out.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(sum))), n * 2);
  }
  return out;
}

interface Clip {
  wav: Buffer;
  base64: string;
  seconds: number;
}

/** The case's clip as production would send it, synthesized on first use. */
async function clipFor(c: Case): Promise<Clip> {
  mkdirSync(AUDIO_DIR, { recursive: true });
  const hash = createHash("sha256").update(`${TTS_MODEL}|${c.voice}|${c.text}`).digest("hex").slice(0, 12);
  const path = join(AUDIO_DIR, `${c.id}-${hash}.wav`);
  if (!existsSync(path)) {
    const response = await fetch("https://openrouter.ai/api/v1/audio/speech", {
      method: "POST",
      headers: { Authorization: `Bearer ${OPENROUTER_KEY}`, "Content-Type": "application/json" },
      body: JSON.stringify({
        model: TTS_MODEL,
        input: c.text,
        voice: `${c.voice}:MAI-Voice-2.1-Flash`,
        response_format: "pcm",
      }),
    });
    if (!response.ok) {
      throw new Error(`Synthesizing ${c.id} failed (${response.status}): ${(await response.text()).slice(0, 300)}`);
    }
    const pcm = Buffer.from(await response.arrayBuffer());
    writeFileSync(path, encodeWav(resample(pcm, TTS_RATE, SAMPLE_RATE)));
  }
  const wav = readFileSync(path);
  return { wav, base64: wav.toString("base64"), seconds: (wav.length - 44) / (SAMPLE_RATE * 2) };
}

interface Attempt {
  ms: number;
  status: number;
  text: string;
  error?: string;
  /** Time the gateway reports spending at Azure, when it reports one. */
  upstreamMs?: number;
  costUsd?: number;
  billedSeconds?: number;
  region?: string;
  /** OpenRouter's id for the request, which its generation API reports timing under. */
  generationId?: string;
}

interface Gateway {
  name: string;
  transcribe(clip: Clip, c: Case): Promise<Attempt>;
}

async function timed(
  request: () => Promise<Response>,
  read: (body: Record<string, unknown>, headers: Headers) => Partial<Attempt>,
): Promise<Attempt> {
  const t0 = performance.now();
  try {
    const response = await request();
    const raw = await response.text();
    const ms = performance.now() - t0;
    if (!response.ok) return { ms, status: response.status, text: "", error: raw.slice(0, 200) };
    const body = JSON.parse(raw) as Record<string, unknown>;
    if (typeof body.text !== "string") return { ms, status: response.status, text: "", error: "no text" };
    return { ms, status: response.status, text: body.text.trim(), ...read(body, response.headers) };
  } catch (err) {
    return { ms: performance.now() - t0, status: 0, text: "", error: err instanceof Error ? err.message : String(err) };
  }
}

/** Mirrors transcriptionRequest in src/stt.ts. */
const openRouter: Gateway = {
  name: "openrouter",
  transcribe: (clip, c) =>
    timed(
      () => {
        const azure: Record<string, unknown> = { enhancedMode: { modelOptions: { transcribeStyle: "clean" } } };
        if (c.terms?.length) azure.phraseList = { phrases: c.terms };
        return fetch("https://openrouter.ai/api/v1/audio/transcriptions", {
          method: "POST",
          headers: {
            Authorization: `Bearer ${OPENROUTER_KEY}`,
            "Content-Type": "application/json",
            "X-Title": "WeldSpeak",
          },
          body: JSON.stringify({
            model: MODEL,
            input_audio: { data: clip.base64, format: "wav" },
            ...(c.language ? { language: c.language } : {}),
            temperature: 0,
            provider: { options: { azure } },
          }),
        });
      },
      (body, headers) => {
        const usage = body.usage as { seconds?: number; cost?: number } | undefined;
        return {
          costUsd: usage?.cost,
          billedSeconds: usage?.seconds,
          region: headers.get("cf-ray")?.split("-")[1],
          generationId: headers.get("x-generation-id") ?? undefined,
        };
      },
    ),
};

/** The same request in the AI SDK's names, which is what the gateway's REST endpoint takes. */
const vercel: Gateway = {
  name: "vercel",
  transcribe: (clip, c) =>
    timed(
      () => {
        const azure: Record<string, unknown> = { transcribeStyle: "clean" };
        if (c.language) azure.locales = [c.language];
        if (c.terms?.length) azure.phraseList = { phrases: c.terms };
        return fetch("https://ai-gateway.vercel.sh/v4/ai/transcription-model", {
          method: "POST",
          headers: {
            Authorization: `Bearer ${GATEWAY_KEY}`,
            "ai-gateway-protocol-version": "0.0.1",
            "ai-transcription-model-specification-version": "4",
            "ai-model-id": MODEL,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({ audio: clip.base64, mediaType: "audio/wav", providerOptions: { azure } }),
        });
      },
      (body, headers) => {
        const gateway = (body.providerMetadata as { gateway?: Record<string, unknown> } | undefined)?.gateway;
        const routing = gateway?.routing as
          | { modelAttempts?: Array<{ providerAttempts?: Array<{ startTime?: number; endTime?: number }> }> }
          | undefined;
        const attempts = routing?.modelAttempts?.flatMap((m) => m.providerAttempts ?? []) ?? [];
        const upstreamMs = attempts.reduce((sum, a) => sum + ((a.endTime ?? 0) - (a.startTime ?? 0)), 0);
        return {
          upstreamMs: attempts.length > 0 ? upstreamMs : undefined,
          costUsd: gateway?.cost === undefined ? undefined : Number(gateway.cost),
          billedSeconds: typeof body.durationInSeconds === "number" ? body.durationInSeconds : undefined,
          region: headers.get("x-vercel-id")?.split("::")[0],
        };
      },
    ),
};

const GATEWAYS = [openRouter, vercel];

/**
 * Fill in the time OpenRouter spent at Azure.
 *
 * Vercel reports it in the response; OpenRouter only in its generation API,
 * a moment after the request.
 */
async function lookUpUpstream(attempts: Attempt[]): Promise<void> {
  const pending = attempts.filter((a) => a.generationId && a.upstreamMs === undefined);
  for (let i = 0; i < pending.length; i += 8) {
    await Promise.all(
      pending.slice(i, i + 8).map(async (a) => {
        try {
          const response = await fetch(`https://openrouter.ai/api/v1/generation?id=${a.generationId}`, {
            headers: { Authorization: `Bearer ${OPENROUTER_KEY}` },
          });
          if (!response.ok) return;
          const body = (await response.json()) as { data?: { latency?: number } };
          if (typeof body.data?.latency === "number") a.upstreamMs = body.data.latency;
        } catch {
          // Timing we can do without.
        }
      }),
    );
  }
}

interface StreamAttempt {
  /** Until the socket is open. */
  connectMs: number;
  /** From the first frame of audio to the first words back. */
  firstTextMs: number;
  /** From the end of the audio to the final text: what is left to wait for on release. */
  tailMs: number;
  text: string;
  warnings: string[];
  error?: string;
}

/**
 * Transcribe a clip over the gateway's streaming socket, fed at speaking pace.
 *
 * The wire format is the AI SDK's: the key rides in a subprotocol because a
 * browser socket cannot set headers, a start frame names the audio format,
 * raw PCM follows in binary frames, and an audio-done frame ends it. The
 * streaming model takes a language hint and nothing else; the phrase list and
 * clean style belong to the batch API and come back as warnings.
 */
function streamOnce(clip: Clip, c: Case): Promise<StreamAttempt> {
  return new Promise((resolve) => {
    const pcm = clip.wav.subarray(44);
    const result: StreamAttempt = { connectMs: NaN, firstTextMs: NaN, tailMs: NaN, text: "", warnings: [] };
    const t0 = performance.now();
    let audioStart = 0;
    let doneAt = 0;
    let settled = false;

    const socket = new WebSocket(
      `wss://ai-gateway.vercel.sh/v4/ai/transcription-model?ai-model-id=${encodeURIComponent(STREAM_MODEL)}`,
      ["ai-gateway-transcription.v1", `ai-gateway-auth.${GATEWAY_KEY}`],
    );
    const settle = (error?: string) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.close();
      resolve(error ? { ...result, error } : result);
    };
    const timer = setTimeout(() => settle("timed out"), clip.seconds * 1000 + 30_000);

    socket.onopen = async () => {
      result.connectMs = performance.now() - t0;
      socket.send(
        JSON.stringify({
          type: "transcription-stream.start",
          inputAudioFormat: { type: "audio/pcm", rate: SAMPLE_RATE },
          ...(c.language ? { providerOptions: { azure: { language: c.language } } } : {}),
        }),
      );
      audioStart = performance.now();
      for (let offset = 0, frame = 1; offset < pcm.length && !settled; offset += FRAME_BYTES, frame++) {
        socket.send(pcm.subarray(offset, offset + FRAME_BYTES));
        // Against the clock rather than frame to frame, so a late timer does not stretch the clip.
        await sleep(Math.max(0, audioStart + frame * FRAME_MS - performance.now()));
      }
      if (settled) return;
      socket.send(JSON.stringify({ type: "transcription-stream.audio-done" }));
      doneAt = performance.now();
    };
    socket.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      const part = JSON.parse(event.data) as {
        type?: string;
        text?: string;
        warnings?: Array<{ feature?: string; message?: string }>;
        error?: { message?: string };
      };
      if (part.type === "stream-start") {
        result.warnings = (part.warnings ?? []).map((w) => w.feature ?? w.message ?? "warning");
      } else if (part.type === "transcript-partial" && Number.isNaN(result.firstTextMs)) {
        result.firstTextMs = performance.now() - audioStart;
      } else if (part.type === "finish") {
        result.tailMs = performance.now() - doneAt;
        result.text = (part.text ?? "").trim();
        settle();
      } else if (part.type === "error") {
        settle(part.error?.message ?? "error");
      }
    };
    socket.onerror = () => settle("socket error");
    socket.onclose = (event) => settle(`closed ${event.code} ${event.reason}`);
  });
}

function words(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s'-]/gu, " ")
    .replace(/[-']/g, "")
    .split(/\s+/)
    .filter(Boolean);
}

/** Word error rate: edits to turn the transcript into the reference, per reference word. */
function wer(reference: string, transcript: string): number {
  const ref = words(reference);
  const hyp = words(transcript);
  let previous = Array.from({ length: hyp.length + 1 }, (_, j) => j);
  for (let i = 1; i <= ref.length; i++) {
    const row = [i];
    for (let j = 1; j <= hyp.length; j++) {
      row[j] = Math.min(previous[j]! + 1, row[j - 1]! + 1, previous[j - 1]! + (ref[i - 1] === hyp[j - 1] ? 0 : 1));
    }
    previous = row;
  }
  return previous[hyp.length]! / Math.max(1, ref.length);
}

function expectations(c: Case, text: string): boolean {
  const lower = text.toLowerCase();
  return (
    (c.include ?? []).every((s) => lower.includes(s.toLowerCase())) &&
    (c.exclude ?? []).every((s) => !`${lower} `.includes(s.toLowerCase()))
  );
}

function pct(values: number[], p: number): number {
  if (values.length === 0) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)]!;
}

const mean = (values: number[]) => values.reduce((a, b) => a + b, 0) / Math.max(1, values.length);
const ms = (value: number) => (Number.isFinite(value) ? value.toFixed(0) : "-");
const sleep = (duration: number) => new Promise((resolve) => setTimeout(resolve, duration));

/** The gateways in an order that alternates, so neither always goes first. */
function ordered(round: number): Gateway[] {
  return round % 2 === 0 ? GATEWAYS : [...GATEWAYS].reverse();
}

function statuses(attempts: Attempt[]): string {
  const counts = new Map<number, number>();
  for (const a of attempts) counts.set(a.status, (counts.get(a.status) ?? 0) + 1);
  return [...counts].map(([status, n]) => `${status}×${n}`).join(" ");
}

async function main() {
  const wanted = process.argv.slice(2);
  const cases = wanted.length > 0 ? CASES.filter((c) => wanted.includes(c.id)) : CASES;
  if (cases.length === 0) throw new Error(`No such case. Known: ${CASES.map((c) => c.id).join(", ")}`);

  const clips = new Map<string, Clip>();
  for (const c of cases) clips.set(c.id, await clipFor(c));
  console.log(`Model: ${MODEL} | cases: ${cases.length} | runs per case: ${RUNS} | pace: ${PACE_MS}ms | clips: ${AUDIO_DIR}\n`);

  /** What the batch model wrote for each case, to hold the streaming model's text against. */
  const batchTexts = new Map<string, Set<string>>();

  if (PHASES.has("cases")) await runCases(cases, clips, batchTexts);

  const piece = cases.find((c) => c.id === "sentence-en") ?? cases[0]!;
  const pieceClip = clips.get(piece.id)!;
  if (PHASES.has("train") && TRAIN > 0) await runTrains(piece, pieceClip);
  if (PHASES.has("burst") && BURST > 0) await runBursts(piece, pieceClip);
  if (PHASES.has("stream") && STREAM_RUNS > 0) await runStream(cases, clips, batchTexts);
}

/** Every case, the gateways taking turns to go first. */
async function runCases(cases: Case[], clips: Map<string, Clip>, batchTexts: Map<string, Set<string>>) {
  const all = new Map<string, Attempt[]>(GATEWAYS.map((g) => [g.name, []]));
  let audioSeconds = 0;
  let round = 0;
  console.log(
    "case".padEnd(14) +
      "audio".padStart(7) +
      GATEWAYS.map((g) => `${g.name} p50`.padStart(16) + "p95".padStart(7) + "wer".padStart(6)).join("") +
      "  same text",
  );

  for (const c of cases) {
    const clip = clips.get(c.id)!;
    audioSeconds += clip.seconds * RUNS;
    const byGateway = new Map<string, Attempt[]>(GATEWAYS.map((g) => [g.name, []]));
    for (let run = 0; run < RUNS; run++) {
      const started = performance.now();
      for (const g of ordered(round++)) {
        const attempt = await g.transcribe(clip, c);
        byGateway.get(g.name)!.push(attempt);
        all.get(g.name)!.push(attempt);
      }
      await sleep(Math.max(0, PACE_MS - (performance.now() - started)));
    }

    let row = c.id.padEnd(14) + `${clip.seconds.toFixed(1)}s`.padStart(7);
    const texts = new Map<string, string[]>();
    const notes: string[] = [];
    for (const g of GATEWAYS) {
      const attempts = byGateway.get(g.name)!;
      const ok = attempts.filter((a) => !a.error);
      const latencies = ok.map((a) => a.ms);
      const score = c.unscored || ok.length === 0 ? NaN : mean(ok.map((a) => wer(c.text, a.text)));
      row += ms(pct(latencies, 50)).padStart(16) + ms(pct(latencies, 95)).padStart(7);
      row += (Number.isFinite(score) ? `${(score * 100).toFixed(0)}%` : "-").padStart(6);
      for (const a of ok) texts.set(a.text, [...new Set([...(texts.get(a.text) ?? []), g.name])]);
      if (g === vercel) batchTexts.set(c.id, new Set(ok.map((a) => a.text)));
      const failure = attempts.find((a) => a.error);
      if (failure) notes.push(`${g.name}: ${attempts.length - ok.length} failed (${failure.status} ${failure.error})`);
      const missed = ok.filter((a) => !expectations(c, a.text)).length;
      if (missed > 0) notes.push(`${g.name}: ${missed}/${ok.length} missed expectations`);
    }
    console.log(`${row}  ${texts.size === 1 ? "yes" : `no (${texts.size} variants)`}`);
    for (const note of notes) console.log(`    ! ${note}`);
    for (const [text, from] of texts) {
      const shown = text.length > 160 ? `${text.slice(0, 160)}…` : text;
      console.log(`    ${texts.size > 1 ? `[${from.join("+")}] ` : ""}${JSON.stringify(shown)}`);
    }
  }

  await lookUpUpstream(all.get(openRouter.name)!);
  console.log(`\n=== All cases${PACE_MS > 0 ? `, ${PACE_MS / 1000}s apart` : ", back to back"} ===\n`);
  console.log(
    "gateway".padEnd(12) +
      "ok".padStart(8) +
      "p50".padStart(7) +
      "p95".padStart(7) +
      "max".padStart(7) +
      "azure p50".padStart(11) +
      "p95".padStart(7) +
      "gateway p50".padStart(13) +
      "p95".padStart(7) +
      "region".padStart(8) +
      "$/audio-hour".padStart(14),
  );
  for (const g of GATEWAYS) {
    const attempts = all.get(g.name)!;
    const ok = attempts.filter((a) => !a.error);
    const latencies = ok.map((a) => a.ms);
    const split = ok.filter((a) => a.upstreamMs !== undefined);
    const azure = split.map((a) => a.upstreamMs!);
    const hop = split.map((a) => a.ms - a.upstreamMs!);
    // Failed requests are not billed, so the rate only holds when all succeeded.
    const cost = ok.reduce((sum, a) => sum + (a.costUsd ?? 0), 0);
    console.log(
      g.name.padEnd(12) +
        `${ok.length}/${attempts.length}`.padStart(8) +
        ms(pct(latencies, 50)).padStart(7) +
        ms(pct(latencies, 95)).padStart(7) +
        ms(Math.max(...latencies)).padStart(7) +
        ms(pct(azure, 50)).padStart(11) +
        ms(pct(azure, 95)).padStart(7) +
        ms(pct(hop, 50)).padStart(13) +
        ms(pct(hop, 95)).padStart(7) +
        (ok[0]?.region ?? "-").padStart(8) +
        (ok.length === attempts.length ? (cost / (audioSeconds / 3600)).toFixed(3) : "-").padStart(14),
    );
    if (ok.length < attempts.length) console.log(`    statuses ${statuses(attempts)}`);
  }
}

/** One after another: the first request finds the connection cold, the rest find it warm. */
async function runTrains(piece: Case, pieceClip: Clip) {
  {
    const first = new Map<string, Attempt[]>(GATEWAYS.map((g) => [g.name, []]));
    const rest = new Map<string, Attempt[]>(GATEWAYS.map((g) => [g.name, []]));
    for (let train = 0; train < 3; train++) {
      await sleep(COOLDOWN_MS);
      for (let i = 0; i < TRAIN; i++) {
        for (const g of ordered(train + i)) {
          (i === 0 ? first : rest).get(g.name)!.push(await g.transcribe(pieceClip, piece));
        }
      }
    }
    console.log(`\n=== ${TRAIN} in a row, three times (${piece.id}, ${pieceClip.seconds.toFixed(1)}s) ===\n`);
    for (const g of GATEWAYS) {
      const cold = first.get(g.name)!.filter((a) => !a.error).map((a) => a.ms);
      const warm = rest.get(g.name)!.filter((a) => !a.error).map((a) => a.ms);
      console.log(
        `${g.name.padEnd(12)} first [${cold.map(ms).join(", ")}]  then p50 ${ms(pct(warm, 50)).padStart(5)}  p95 ${ms(pct(warm, 95)).padStart(5)}  statuses ${statuses([...first.get(g.name)!, ...rest.get(g.name)!])}`,
      );
    }
  }

}

/** The pieces of one dictation in flight together. */
async function runBursts(piece: Case, pieceClip: Clip) {
  {
    const bursts = new Map<string, Attempt[]>(GATEWAYS.map((g) => [g.name, []]));
    for (let burst = 0; burst < 3; burst++) {
      await sleep(COOLDOWN_MS);
      for (const g of ordered(burst)) {
        bursts.get(g.name)!.push(...(await Promise.all(Array.from({ length: BURST }, () => g.transcribe(pieceClip, piece)))));
      }
    }
    console.log(`\n=== ${BURST} at once, three times (${piece.id}) ===\n`);
    for (const g of GATEWAYS) {
      const attempts = bursts.get(g.name)!;
      const ok = attempts.filter((a) => !a.error).map((a) => a.ms);
      console.log(
        `${g.name.padEnd(12)} p50 ${ms(pct(ok, 50)).padStart(5)}  p95 ${ms(pct(ok, 95)).padStart(5)}  max ${ms(Math.max(...ok)).padStart(5)}  statuses ${statuses(attempts)}`,
      );
    }
  }
}

/**
 * The streaming model, one dictation at a time and then several at once.
 *
 * What matters is the tail: with the audio already sent while it was spoken,
 * the wait after release no longer grows with the length of the last piece.
 */
async function runStream(cases: Case[], clips: Map<string, Clip>, batchTexts: Map<string, Set<string>>) {
  console.log(`
=== ${STREAM_MODEL} through vercel, fed at speaking pace, ${STREAM_RUNS} runs ===
`);
  console.log(
    "case".padEnd(14) + "audio".padStart(7) + "connect".padStart(9) + "first text".padStart(12) + "tail p50".padStart(10) + "max".padStart(7) + "wer".padStart(6) + "  same as batch",
  );
  const tails: number[] = [];
  for (const c of cases) {
    const clip = clips.get(c.id)!;
    const attempts: StreamAttempt[] = [];
    for (let run = 0; run < STREAM_RUNS; run++) attempts.push(await streamOnce(clip, c));
    const ok = attempts.filter((a) => !a.error);
    tails.push(...ok.map((a) => a.tailMs));
    const score = c.unscored || ok.length === 0 ? NaN : mean(ok.map((a) => wer(c.text, a.text)));
    const texts = new Set(ok.map((a) => a.text));
    const batch = batchTexts.get(c.id);
    const same = !batch || batch.size === 0 ? "-" : [...texts].every((t) => batch.has(t)) ? "yes" : "no";
    console.log(
      c.id.padEnd(14) +
        `${clip.seconds.toFixed(1)}s`.padStart(7) +
        ms(pct(ok.map((a) => a.connectMs), 50)).padStart(9) +
        ms(pct(ok.map((a) => a.firstTextMs), 50)).padStart(12) +
        ms(pct(ok.map((a) => a.tailMs), 50)).padStart(10) +
        ms(Math.max(...ok.map((a) => a.tailMs))).padStart(7) +
        (Number.isFinite(score) ? `${(score * 100).toFixed(0)}%` : "-").padStart(6) +
        `  ${same}`,
    );
    const failure = attempts.find((a) => a.error);
    if (failure) console.log(`    ! ${attempts.length - ok.length} failed (${failure.error})`);
    const warnings = new Set(ok.flatMap((a) => a.warnings));
    if (warnings.size > 0) console.log(`    ! unsupported: ${[...warnings].join(", ")}`);
    const missed = ok.filter((a) => !expectations(c, a.text)).length;
    if (missed > 0) console.log(`    ! ${missed}/${ok.length} missed expectations`);
    for (const text of texts) console.log(`    ${JSON.stringify(text.length > 160 ? `${text.slice(0, 160)}…` : text)}`);
  }
  console.log(`
tail p50 ${ms(pct(tails, 50))}  p95 ${ms(pct(tails, 95))}  max ${ms(Math.max(...tails))}`);

  if (BURST > 0) {
    const piece = cases.find((c) => c.id === "sentence-en") ?? cases[0]!;
    const attempts = await Promise.all(Array.from({ length: BURST }, () => streamOnce(clips.get(piece.id)!, piece)));
    const ok = attempts.filter((a) => !a.error);
    const failures = [...new Set(attempts.filter((a) => a.error).map((a) => a.error))];
    console.log(
      `${BURST} streams at once (${piece.id}): ${ok.length} ok, tail p50 ${ms(pct(ok.map((a) => a.tailMs), 50))}  max ${ms(Math.max(...ok.map((a) => a.tailMs)))}${failures.length > 0 ? `  failures: ${failures.join("; ")}` : ""}`,
    );
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
