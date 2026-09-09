/**
 * Phase 1 checkpoint: stream a WAV file through the dictation WebSocket.
 *
 * This proves the whole Cloudflare path — auth, Durable Object, Workers AI
 * relay, cleanup — with no desktop code involved. Run it before writing any
 * Rust, and again whenever the upstream model's options are suspected of
 * having drifted.
 *
 *   pnpm --filter @weldspeak/api test:stream ./fixtures/hello.wav
 *
 * Requires WELDSPEAK_TOKEN (a desktop access token; approve a device against a
 * local or deployed Worker to get one) and optionally WELDSPEAK_API
 * (default http://localhost:8787) and WELDSPEAK_ORG.
 */

import { readFileSync } from "node:fs";
import { FRAME_BYTES, FRAME_MS, SAMPLE_RATE, encodeTokenSubprotocol } from "@weldspeak/protocol";
import type { ServerEvent } from "@weldspeak/protocol";

interface Wav {
  sampleRate: number;
  channels: number;
  bitsPerSample: number;
  pcm: Buffer;
}

/**
 * Parse a PCM WAV.
 *
 * Chunks are walked rather than assumed at fixed offsets: recorders routinely
 * insert LIST/fact chunks before `data`, and a hardcoded 44-byte header would
 * silently feed metadata to the recognizer as audio.
 */
function parseWav(buffer: Buffer): Wav {
  if (buffer.toString("ascii", 0, 4) !== "RIFF" || buffer.toString("ascii", 8, 12) !== "WAVE") {
    throw new Error("Not a RIFF/WAVE file");
  }

  let offset = 12;
  let fmt: { audioFormat: number; channels: number; sampleRate: number; bits: number } | null =
    null;

  while (offset + 8 <= buffer.length) {
    const id = buffer.toString("ascii", offset, offset + 4);
    const size = buffer.readUInt32LE(offset + 4);
    const body = offset + 8;

    if (id === "fmt ") {
      fmt = {
        audioFormat: buffer.readUInt16LE(body),
        channels: buffer.readUInt16LE(body + 2),
        sampleRate: buffer.readUInt32LE(body + 4),
        bits: buffer.readUInt16LE(body + 14),
      };
    } else if (id === "data") {
      if (!fmt) throw new Error("data chunk before fmt chunk");
      if (fmt.audioFormat !== 1) {
        throw new Error(`Expected uncompressed PCM (format 1), got ${fmt.audioFormat}`);
      }
      return {
        sampleRate: fmt.sampleRate,
        channels: fmt.channels,
        bitsPerSample: fmt.bits,
        pcm: buffer.subarray(body, body + size),
      };
    }

    // Chunks are word-aligned: an odd size is followed by a pad byte.
    offset = body + size + (size % 2);
  }

  throw new Error("No data chunk found");
}

async function main(): Promise<void> {
  const path = process.argv[2];
  if (!path) {
    console.error("usage: stream-wav.ts <file.wav>");
    process.exit(2);
  }

  const token = process.env.WELDSPEAK_TOKEN;
  if (!token) {
    console.error("WELDSPEAK_TOKEN is required (a desktop access token)");
    process.exit(2);
  }

  const api = process.env.WELDSPEAK_API ?? "http://localhost:8787";
  const org = process.env.WELDSPEAK_ORG;

  const wav = parseWav(readFileSync(path));

  // The Worker rejects a mismatched rate rather than resampling: in production
  // the desktop client resamples, and silently accepting anything here would
  // hide a real client bug.
  if (wav.sampleRate !== SAMPLE_RATE || wav.channels !== 1 || wav.bitsPerSample !== 16) {
    console.error(
      `Expected ${SAMPLE_RATE} Hz mono 16-bit, got ${wav.sampleRate} Hz ` +
        `${wav.channels}ch ${wav.bitsPerSample}-bit.\n` +
        `Convert with: ffmpeg -i ${path} -ar ${SAMPLE_RATE} -ac 1 -c:a pcm_s16le out.wav`,
    );
    process.exit(2);
  }

  const wsUrl = new URL("/v1/stream", api.replace(/^http/, "ws"));
  if (org) wsUrl.searchParams.set("org", org);

  console.log(`→ ${wsUrl.href} (${(wav.pcm.length / (SAMPLE_RATE * 2)).toFixed(1)}s of audio)`);

  const socket = new WebSocket(wsUrl.href, [encodeTokenSubprotocol(token)]);
  const startedAt = Date.now();
  let firstPartialAt: number | null = null;

  socket.addEventListener("open", () => {
    socket.send(
      JSON.stringify({
        type: "start",
        sampleRate: SAMPLE_RATE,
        encoding: "linear16",
        appName: "stream-wav",
        ...(org ? { orgId: org } : {}),
      }),
    );
  });

  socket.addEventListener("message", async (event) => {
    const message = JSON.parse(String(event.data)) as ServerEvent;

    switch (message.type) {
      case "ready":
        console.log(`ready (${message.sessionId})`);
        await streamAudio(socket, wav.pcm);
        socket.send(JSON.stringify({ type: "stop" }));
        break;

      case "partial":
        firstPartialAt ??= Date.now();
        process.stdout.write(`\r  partial: ${message.text.slice(-80)}`);
        break;

      case "transcript":
        console.log(`\n  raw:     ${message.text}`);
        break;

      case "result":
        console.log(`  result:  ${message.text}`);
        console.log(`  formatted: ${message.formatted}`);
        if (firstPartialAt) {
          console.log(`  first partial after ${firstPartialAt - startedAt} ms`);
        }
        console.log(`  total ${Date.now() - startedAt} ms`);
        socket.close();
        process.exit(message.text ? 0 : 1);
        break;

      case "error":
        console.error(`\nerror [${message.code}] ${message.message}`);
        socket.close();
        process.exit(1);
    }
  });

  socket.addEventListener("error", () => {
    console.error("socket error — is the Worker running, and the token valid?");
    process.exit(1);
  });
}

/**
 * Send the file in real time rather than as fast as possible.
 *
 * A streaming recognizer's endpointing is tuned for live speech; firing a whole
 * file at it in one burst produces timings nothing like production.
 */
async function streamAudio(socket: WebSocket, pcm: Buffer): Promise<void> {
  for (let offset = 0; offset < pcm.length; offset += FRAME_BYTES) {
    const frame = pcm.subarray(offset, Math.min(offset + FRAME_BYTES, pcm.length));
    socket.send(frame);
    await new Promise((resolve) => setTimeout(resolve, FRAME_MS));
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
