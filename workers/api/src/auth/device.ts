/**
 * OAuth 2.0 device authorization grant.
 *
 * The desktop app cannot host a Clerk session, so it hands the user off to a
 * real browser: it starts a grant, shows a short code, and polls. The user
 * signs in with Clerk on the dashboard, confirms the code, and the Worker
 * mints desktop tokens on the next poll.
 *
 * Two KV entries back one grant: the device code (secret, held by the app) and
 * the user code (short, typed or read by the human). Both expire on their own,
 * so nothing needs sweeping.
 */

import { Hono } from "hono";
import type {
  DeviceApproveRequest,
  DevicePollRequest,
  DevicePollResponse,
  DeviceStartRequest,
  DeviceStartResponse,
  TokenPair,
} from "@weldspeak/protocol";
import type { AppBindings, AppContext } from "./middleware.js";
import { requireAuth } from "./middleware.js";
import { listOrgMemberships } from "./clerk.js";
import { resolveEntitlement } from "../billing/entitlements.js";
import {
  generateRefreshToken,
  hashRefreshToken,
  mintAccessToken,
  REFRESH_TOKEN_TTL_SECONDS,
} from "./tokens.js";

/** How long the user has to approve before the grant expires. */
const GRANT_TTL_SECONDS = 600; // 10 minutes

/** Minimum seconds between polls. Faster polling gets `slow_down`. */
const POLL_INTERVAL_SECONDS = 2;

/**
 * User-code alphabet, with 0/O/1/I/L removed.
 *
 * The user reads this off one screen and confirms it on another, so
 * characters that look alike cause real failures.
 */
const CODE_ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789";

interface GrantState {
  userCode: string;
  platform: string;
  label: string;
  status: "pending" | "approved" | "denied";
  /** Set once approved. */
  userId?: string;
  deviceId?: string;
  /** Unix seconds of the last poll, used to enforce the poll interval. */
  lastPolledAt?: number;
}

function randomCode(length: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(length));
  return Array.from(bytes, (byte) => CODE_ALPHABET[byte % CODE_ALPHABET.length]).join("");
}

/** Format as `WXYZ-1234` — two short groups are easier to read back than eight run-on characters. */
function generateUserCode(): string {
  return `${randomCode(4)}-${randomCode(4)}`;
}

function generateDeviceCode(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

const grantKey = (deviceCode: string) => `grant:${deviceCode}`;
const userCodeKey = (userCode: string) => `usercode:${userCode.toUpperCase()}`;

/**
 * Issue the tokens for an approved grant and record the device.
 *
 * Tokens are minted at poll time rather than at approval time so they never
 * sit at rest in KV.
 */
async function issueTokens(
  c: AppContext,
  userId: string,
  deviceId: string,
): Promise<TokenPair> {
  const [orgs, entitlement] = await Promise.all([
    listOrgMemberships(c.env, userId),
    resolveEntitlement(c.env, userId),
  ]);
  const { token: accessToken, expiresIn } = await mintAccessToken(
    c.env,
    userId,
    deviceId,
    orgs,
    entitlement,
  );

  const refreshToken = generateRefreshToken();
  const expiresAt = new Date(Date.now() + REFRESH_TOKEN_TTL_SECONDS * 1000).toISOString();

  await c.env.DB.prepare(
    `INSERT INTO refresh_tokens (token_hash, device_id, clerk_user_id, expires_at)
     VALUES (?, ?, ?, ?)`,
  )
    .bind(await hashRefreshToken(refreshToken), deviceId, userId, expiresAt)
    .run();

  return { accessToken, refreshToken, expiresIn };
}

export const deviceRoutes = new Hono<AppBindings>();

/** Begin a grant. Called by the desktop app; no authentication yet. */
deviceRoutes.post("/start", async (c) => {
  const body = await c.req.json<DeviceStartRequest>().catch(() => null);
  if (!body?.platform || !body?.label) {
    return c.json({ error: "bad_request", message: "platform and label are required" }, 400);
  }

  const deviceCode = generateDeviceCode();
  const userCode = generateUserCode();

  const state: GrantState = {
    userCode,
    platform: body.platform.slice(0, 32),
    label: body.label.slice(0, 128),
    status: "pending",
  };

  await Promise.all([
    c.env.DEVICE_CODES.put(grantKey(deviceCode), JSON.stringify(state), {
      expirationTtl: GRANT_TTL_SECONDS,
    }),
    c.env.DEVICE_CODES.put(userCodeKey(userCode), deviceCode, {
      expirationTtl: GRANT_TTL_SECONDS,
    }),
  ]);

  const response: DeviceStartResponse = {
    deviceCode,
    userCode,
    verifyUrl: `${c.env.APP_URL}/link?code=${encodeURIComponent(userCode)}`,
    expiresIn: GRANT_TTL_SECONDS,
    interval: POLL_INTERVAL_SECONDS,
  };
  return c.json(response);
});

/** Poll a grant. Returns tokens once the user has approved in the browser. */
deviceRoutes.post("/poll", async (c) => {
  const body = await c.req.json<DevicePollRequest>().catch(() => null);
  if (!body?.deviceCode) {
    return c.json({ error: "bad_request", message: "deviceCode is required" }, 400);
  }

  const raw = await c.env.DEVICE_CODES.get(grantKey(body.deviceCode));
  if (!raw) {
    // Absent means expired: KV removed it at TTL. An unknown code is
    // indistinguishable from an expired one, which is fine — both mean
    // "start over".
    return c.json<DevicePollResponse>({ status: "expired" });
  }

  const state = JSON.parse(raw) as GrantState;
  const now = Math.floor(Date.now() / 1000);

  if (state.lastPolledAt && now - state.lastPolledAt < POLL_INTERVAL_SECONDS) {
    return c.json<DevicePollResponse>({ status: "slow_down", interval: POLL_INTERVAL_SECONDS });
  }

  if (state.status === "denied") {
    await c.env.DEVICE_CODES.delete(grantKey(body.deviceCode));
    return c.json<DevicePollResponse>({ status: "denied" });
  }

  if (state.status === "pending") {
    await c.env.DEVICE_CODES.put(
      grantKey(body.deviceCode),
      JSON.stringify({ ...state, lastPolledAt: now }),
      { expirationTtl: GRANT_TTL_SECONDS },
    );
    return c.json<DevicePollResponse>({ status: "authorization_pending" });
  }

  if (!state.userId || !state.deviceId) {
    return c.json({ error: "internal", message: "Approved grant is missing its user" }, 500);
  }

  const tokens = await issueTokens(c, state.userId, state.deviceId);

  // One-shot: consume the grant so a leaked device code cannot be replayed.
  await Promise.all([
    c.env.DEVICE_CODES.delete(grantKey(body.deviceCode)),
    c.env.DEVICE_CODES.delete(userCodeKey(state.userCode)),
  ]);

  return c.json<DevicePollResponse>({ status: "approved", tokens });
});

/**
 * Approve a grant. Called from the browser with a Clerk session token — this
 * is the step that binds a Clerk identity to a desktop install.
 */
deviceRoutes.post("/approve", requireAuth(), async (c) => {
  const body = await c.req.json<DeviceApproveRequest>().catch(() => null);
  if (!body?.userCode) {
    return c.json({ error: "bad_request", message: "userCode is required" }, 400);
  }

  const deviceCode = await c.env.DEVICE_CODES.get(userCodeKey(body.userCode));
  if (!deviceCode) {
    return c.json({ error: "not_found", message: "That code has expired or does not exist" }, 404);
  }

  const raw = await c.env.DEVICE_CODES.get(grantKey(deviceCode));
  if (!raw) {
    return c.json({ error: "not_found", message: "That code has expired or does not exist" }, 404);
  }

  const state = JSON.parse(raw) as GrantState;
  if (state.status !== "pending") {
    return c.json({ error: "bad_request", message: "That code has already been used" }, 400);
  }

  const { userId } = c.get("auth");
  const deviceId = crypto.randomUUID();

  await c.env.DB.prepare(
    `INSERT INTO devices (id, clerk_user_id, platform, label) VALUES (?, ?, ?, ?)`,
  )
    .bind(deviceId, userId, state.platform, state.label)
    .run();

  await c.env.DEVICE_CODES.put(
    grantKey(deviceCode),
    JSON.stringify({ ...state, status: "approved", userId, deviceId } satisfies GrantState),
    { expirationTtl: GRANT_TTL_SECONDS },
  );

  return c.json({ ok: true, device: { platform: state.platform, label: state.label } });
});

/** Deny a grant, so a code the user did not initiate can be shut down promptly. */
deviceRoutes.post("/deny", requireAuth(), async (c) => {
  const body = await c.req.json<DeviceApproveRequest>().catch(() => null);
  if (!body?.userCode) {
    return c.json({ error: "bad_request", message: "userCode is required" }, 400);
  }

  const deviceCode = await c.env.DEVICE_CODES.get(userCodeKey(body.userCode));
  if (!deviceCode) return c.json({ ok: true });

  const raw = await c.env.DEVICE_CODES.get(grantKey(deviceCode));
  if (raw) {
    const state = JSON.parse(raw) as GrantState;
    await c.env.DEVICE_CODES.put(
      grantKey(deviceCode),
      JSON.stringify({ ...state, status: "denied" } satisfies GrantState),
      { expirationTtl: GRANT_TTL_SECONDS },
    );
  }
  return c.json({ ok: true });
});
