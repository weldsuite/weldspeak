/**
 * The dictation WebSocket protocol.
 *
 * One socket carries both control and audio: JSON text frames for control,
 * binary frames for raw `linear16` PCM (see ./audio.ts). Binary frames are
 * only meaningful between a `start` and a `stop`/`cancel`.
 *
 * A session is one utterance. The client opens the socket on hotkey-down and
 * closes it after the `result` arrives, so TLS setup and Durable Object
 * wake-up overlap with the user's first words instead of adding to latency.
 */

/** Sent by the client to open a dictation session. Must precede any audio. */
export interface StartFrame {
  type: "start";
  /** Must equal SAMPLE_RATE; sent explicitly so the server can reject a mismatch. */
  sampleRate: number;
  encoding: "linear16";
  /** BCP-47-ish language hint. Omit to let the model detect. */
  locale?: string;
  /**
   * Active organization. The server independently verifies the caller is a
   * member of this org; a client-supplied value is never trusted on its own.
   */
  orgId?: string | null;
  /**
   * Vocabulary boosts — the user's personal terms merged with their org's
   * shared glossary. Passed to the recognizer and reused as spelling context
   * for the cleanup pass.
   */
  keyterms?: string[];
  /**
   * Focused application, for history and as a cleanup style hint (code
   * editor, email, chat). Never used for routing or authorization.
   */
  appName?: string | null;
  /** Run the cleanup pass. When false the raw transcript is returned as-is. */
  format?: boolean;
  /**
   * Store this transcript in the user's history. The person can only opt
   * out: an org that turned retention off still wins when this is true.
   */
  retain?: boolean;
}

/**
 * What surrounds the cursor in the field being dictated into, read when the
 * hotkey went down. Wispr Flow–style context: cleanup uses it to continue a
 * sentence, spell names already on screen, and tell a Gmail tab from a
 * Slack one. Used for this dictation's cleanup only; never stored.
 */
export interface FieldContext {
  /** Text just before the cursor, at most MAX_CONTEXT_BEFORE characters. */
  before?: string;
  /** Text just after the cursor, at most MAX_CONTEXT_AFTER characters. */
  after?: string;
  /** Focused window title — in a browser it names the site ("Inbox - Gmail"). */
  windowTitle?: string;
}

export const MAX_CONTEXT_BEFORE = 1_500;
export const MAX_CONTEXT_AFTER = 500;
export const MAX_WINDOW_TITLE = 200;

/** Sent on hotkey release. The server finalizes, cleans up, and replies `result`. */
export interface StopFrame {
  type: "stop";
  /**
   * Cursor context, captured at hotkey-down and sent here because it is only
   * needed for cleanup. Servers that predate it ignore the field.
   */
  context?: FieldContext;
}

/** Sent on Escape. The server discards the utterance and emits no `result`. */
export interface CancelFrame {
  type: "cancel";
}

/** Keeps intermediaries from idling the socket out during a long pause. */
export interface PingFrame {
  type: "ping";
}

export type ClientFrame = StartFrame | StopFrame | CancelFrame | PingFrame;

/** Server accepted the `start` and is ready for audio. */
export interface ReadyEvent {
  type: "ready";
  sessionId: string;
}

/**
 * Interim recognition result. Overwrites the previous partial rather than
 * appending. Display-only — never inject a partial, it will be revised.
 */
export interface PartialEvent {
  type: "partial";
  text: string;
}

/** The recognizer's final, before cleanup. Emitted even when formatting runs. */
export interface TranscriptEvent {
  type: "transcript";
  text: string;
}

/** Terminal success. This is the text to inject. */
export interface ResultEvent {
  type: "result";
  /** Text to inject: cleaned when `formatted`, otherwise identical to `raw`. */
  text: string;
  /** The recognizer's output before cleanup, retained for history and debugging. */
  raw: string;
  /**
   * Whether cleanup actually ran. False when disabled, when it failed, or when
   * it exceeded its deadline and the raw transcript was shipped instead.
   */
  formatted: boolean;
  /** Audio duration, used for metering. */
  durationMs: number;
}

export type ErrorCode =
  | "unauthorized"
  | "org_forbidden"
  | "quota_exceeded"
  | "bad_request"
  | "upstream_failed"
  | "internal";

/** Terminal failure. `retryable` distinguishes a blip from a wasted retry. */
export interface ErrorEvent {
  type: "error";
  code: ErrorCode;
  message: string;
  retryable: boolean;
}

export interface PongEvent {
  type: "pong";
}

export type ServerEvent =
  | ReadyEvent
  | PartialEvent
  | TranscriptEvent
  | ResultEvent
  | ErrorEvent
  | PongEvent;

/**
 * The JWT is passed as a WebSocket subprotocol rather than a header.
 *
 * Browsers cannot set headers on a WebSocket handshake. A Rust client can, but
 * using the subprotocol keeps a future browser client possible without a
 * second auth path on the server.
 */
export const WS_SUBPROTOCOL_PREFIX = "weldspeak.token.";

/** Build the subprotocol value carrying `token`. */
export function encodeTokenSubprotocol(token: string): string {
  return `${WS_SUBPROTOCOL_PREFIX}${token}`;
}

/** Recover a token from a `Sec-WebSocket-Protocol` list, or null if absent. */
export function decodeTokenSubprotocol(header: string | null): string | null {
  if (!header) return null;
  for (const raw of header.split(",")) {
    const value = raw.trim();
    if (value.startsWith(WS_SUBPROTOCOL_PREFIX)) {
      const token = value.slice(WS_SUBPROTOCOL_PREFIX.length);
      return token.length > 0 ? token : null;
    }
  }
  return null;
}

/**
 * Narrow an untrusted parsed JSON value to a ClientFrame.
 *
 * Frames arrive from the network, so this validates shape rather than
 * asserting it. Returns null on anything unrecognized; callers reply with a
 * `bad_request` error rather than throwing.
 */
/**
 * Validate cursor context from the client.
 *
 * Clamped rather than rejected: context is a hint, and an oversized field
 * from a large document should cost a trim, not the dictation. The text
 * nearest the cursor is what matters, so `before` keeps its end and `after`
 * its start.
 */
export function parseFieldContext(value: unknown): FieldContext | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const raw = value as Record<string, unknown>;
  const text = (field: unknown) => (typeof field === "string" && field.trim() ? field : undefined);

  const before = text(raw.before)?.slice(-MAX_CONTEXT_BEFORE);
  const after = text(raw.after)?.slice(0, MAX_CONTEXT_AFTER);
  const windowTitle = text(raw.windowTitle)?.trim().slice(0, MAX_WINDOW_TITLE);

  if (!before && !after && !windowTitle) return undefined;
  return {
    ...(before ? { before } : {}),
    ...(after ? { after } : {}),
    ...(windowTitle ? { windowTitle } : {}),
  };
}

export function parseClientFrame(value: unknown): ClientFrame | null {
  if (typeof value !== "object" || value === null) return null;
  const frame = value as Record<string, unknown>;

  switch (frame.type) {
    case "stop": {
      const context = parseFieldContext(frame.context);
      return context ? { type: "stop", context } : { type: "stop" };
    }
    case "cancel":
      return { type: "cancel" };
    case "ping":
      return { type: "ping" };
    case "start": {
      if (typeof frame.sampleRate !== "number") return null;
      if (frame.encoding !== "linear16") return null;

      const keyterms = Array.isArray(frame.keyterms)
        ? frame.keyterms.filter((t): t is string => typeof t === "string")
        : undefined;

      return {
        type: "start",
        sampleRate: frame.sampleRate,
        encoding: "linear16",
        locale: typeof frame.locale === "string" ? frame.locale : undefined,
        orgId: typeof frame.orgId === "string" ? frame.orgId : null,
        keyterms,
        appName: typeof frame.appName === "string" ? frame.appName : null,
        format: typeof frame.format === "boolean" ? frame.format : true,
        retain: typeof frame.retain === "boolean" ? frame.retain : true,
      };
    }
    default:
      return null;
  }
}
