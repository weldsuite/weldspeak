/**
 * HTTP API types shared by the desktop client, the web dashboard, and the Worker.
 */

/** Clerk organization roles. Clerk namespaces its built-in roles with `org:`. */
export type OrgRole = "org:admin" | "org:member";

/** One of the caller's organization memberships. */
export interface OrgMembership {
  orgId: string;
  name: string;
  slug: string | null;
  role: OrgRole;
}

// --- Device authorization grant -------------------------------------------
//
// Clerk sessions are browser-bound and short-lived, so the desktop app cannot
// hold one. Instead it runs an OAuth 2.0 device grant against the Worker: the
// user approves in a real browser where Clerk works normally, and the Worker
// mints its own longer-lived tokens for the desktop client.

export interface DeviceStartRequest {
  /** "macos" | "windows" — recorded so the user can tell devices apart. */
  platform: string;
  /** Human-readable device label, e.g. the hostname. */
  label: string;
}

export interface DeviceStartResponse {
  /** Secret held by the desktop app and used to poll. Never shown to the user. */
  deviceCode: string;
  /** Short code the user confirms in the browser, e.g. "WXYZ-1234". */
  userCode: string;
  /** Where to send the user's browser. */
  verifyUrl: string;
  /** Seconds until this attempt expires. */
  expiresIn: number;
  /** Minimum seconds between polls. Clients that poll faster get `slow_down`. */
  interval: number;
}

export interface DevicePollRequest {
  deviceCode: string;
}

export type DevicePollResponse =
  | { status: "authorization_pending" }
  | { status: "slow_down"; interval: number }
  | { status: "expired" }
  | { status: "denied" }
  | { status: "approved"; tokens: TokenPair };

export interface DeviceApproveRequest {
  userCode: string;
}

export interface TokenPair {
  accessToken: string;
  /** Opaque, single-use. Rotated on every refresh. */
  refreshToken: string;
  /** Access token lifetime in seconds. */
  expiresIn: number;
}

export interface RefreshRequest {
  refreshToken: string;
}

// --- Identity --------------------------------------------------------------

export interface MeResponse {
  userId: string;
  email: string | null;
  displayName: string | null;
  imageUrl: string | null;
  /** Every org the caller belongs to, so the client can offer a switcher. */
  orgs: OrgMembership[];
}

// --- Dictionary ------------------------------------------------------------

/**
 * `user` terms are private to their owner. `org` terms are the shared team
 * glossary — readable by every member, writable only by `org:admin`. Both sets
 * are merged for a dictation.
 */
export type TermScope = "user" | "org";

export interface DictionaryTerm {
  id: string;
  scope: TermScope;
  /** The correct spelling, e.g. "Inconel 625". */
  term: string;
  /** Optional phonetic hint for terms the recognizer mangles. */
  soundsLike: string | null;
  createdAt: string;
}

export interface CreateTermRequest {
  scope: TermScope;
  term: string;
  soundsLike?: string | null;
}

// --- Transcripts -----------------------------------------------------------

export interface TranscriptRecord {
  id: string;
  raw: string;
  formatted: string;
  durationMs: number;
  appName: string | null;
  createdAt: string;
}

// --- Org settings and usage ------------------------------------------------

export interface OrgSettings {
  orgId: string;
  /**
   * When false, an admin has disabled transcript storage for the whole org.
   * Clients must honour this locally too, not merely skip the server sync.
   */
  retainTranscripts: boolean;
  /** Monthly cap on dictation minutes for the org. Null means uncapped. */
  monthlyMinuteCap: number | null;
}

export interface UsageSummary {
  orgId: string | null;
  /** First day of the reported period, ISO date. */
  periodStart: string;
  audioSeconds: number;
  monthlyMinuteCap: number | null;
  /** Per-member breakdown. Admin-only; empty for non-admins. */
  byUser: Array<{ userId: string; audioSeconds: number }>;
}

export interface ApiError {
  error: string;
  message: string;
}
