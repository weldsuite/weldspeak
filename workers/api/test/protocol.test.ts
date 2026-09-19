/**
 * Wire protocol.
 *
 * The TypeScript and Rust definitions are kept in step by hand, so these tests
 * pin the constants and the parsing behaviour the Rust side asserts against in
 * `packages/protocol-rs`. If someone changes one side only, one of the two
 * suites fails.
 */

import { describe, expect, it } from "vitest";
import {
  decodeTokenSubprotocol,
  encodeTokenSubprotocol,
  parseClientFrame,
  bytesToMs,
  FRAME_BYTES,
  FRAME_SAMPLES,
  PREROLL_FRAMES,
  SAMPLE_RATE,
} from "@weldspeak/protocol";

describe("audio constants", () => {
  it("describes a 20 ms frame of 16 kHz mono linear16", () => {
    expect(SAMPLE_RATE).toBe(16_000);
    expect(FRAME_SAMPLES).toBe(320);
    expect(FRAME_BYTES).toBe(640);
    expect(PREROLL_FRAMES).toBe(25);
  });

  it("converts bytes to duration", () => {
    expect(bytesToMs(SAMPLE_RATE * 2)).toBe(1000);
    expect(bytesToMs(FRAME_BYTES)).toBe(20);
  });
});

describe("client frame parsing", () => {
  it("accepts a well-formed start frame", () => {
    const frame = parseClientFrame({
      type: "start",
      sampleRate: 16_000,
      encoding: "linear16",
      locale: "en",
      orgId: "org_acme",
      keyterms: ["Inconel 625"],
    });

    expect(frame).toMatchObject({ type: "start", orgId: "org_acme" });
  });

  it("defaults formatting on when the client does not say", () => {
    const frame = parseClientFrame({ type: "start", sampleRate: 16_000, encoding: "linear16" });
    expect(frame).toMatchObject({ format: true });
  });

  it("drops non-string entries from keyterms", () => {
    // Frames arrive from the network, so the list is filtered rather than trusted.
    const frame = parseClientFrame({
      type: "start",
      sampleRate: 16_000,
      encoding: "linear16",
      keyterms: ["good", 42, null, "also good"],
    });

    expect(frame).toMatchObject({ keyterms: ["good", "also good"] });
  });

  it("rejects an unsupported encoding", () => {
    expect(
      parseClientFrame({ type: "start", sampleRate: 16_000, encoding: "mp3" }),
    ).toBeNull();
  });

  it("rejects a start frame with no sample rate", () => {
    expect(parseClientFrame({ type: "start", encoding: "linear16" })).toBeNull();
  });

  it("accepts the unit control frames", () => {
    expect(parseClientFrame({ type: "stop" })).toEqual({ type: "stop" });
    expect(parseClientFrame({ type: "cancel" })).toEqual({ type: "cancel" });
    expect(parseClientFrame({ type: "ping" })).toEqual({ type: "ping" });
  });

  it("rejects anything unrecognised", () => {
    for (const bad of [null, undefined, 42, "start", [], {}, { type: "explode" }]) {
      expect(parseClientFrame(bad)).toBeNull();
    }
  });
});

describe("token subprotocol", () => {
  it("round-trips a token", () => {
    expect(decodeTokenSubprotocol(encodeTokenSubprotocol("abc.def.ghi"))).toBe("abc.def.ghi");
  });

  it("finds the token among other offered subprotocols", () => {
    const header = `chat, ${encodeTokenSubprotocol("tok")}, superchat`;
    expect(decodeTokenSubprotocol(header)).toBe("tok");
  });

  it("returns null when there is no token", () => {
    expect(decodeTokenSubprotocol(null)).toBeNull();
    expect(decodeTokenSubprotocol("chat, superchat")).toBeNull();
    expect(decodeTokenSubprotocol("weldspeak.token.")).toBeNull();
  });
});
