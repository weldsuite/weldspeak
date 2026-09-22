/**
 * WeldSpeak-issued tokens for the desktop client.
 *
 * Clerk is the identity source of truth, but a Clerk session token lives about
 * a minute and is refreshed through browser cookies on our own domain — no use
 * to a desktop app. So once Clerk has vouched for the user in a browser, the
 * Worker mints its own pair: a short-lived signed access token the client
 * sends on every request, and an opaque refresh token it exchanges for a new
 * pair before expiry.
 */

import type { OrgMembership } from "@weldspeak/protocol";
import type { Env } from "../env.js";
import type { Entitlement } from "../billing/entitlements.js";

/** Access token lifetime. Short enough that a revoked membership stops
 *  working quickly, long enough not to churn on a laptop that sleeps. */
export const ACCESS_TOKEN_TTL_SECONDS = 60 * 60; // 1 hour

/** Refresh token lifetime. Past this the user re-runs the device flow. */
export const REFRESH_TOKEN_TTL_SECONDS = 60 * 60 * 24 * 60; // 60 days

export const TOKEN_ISSUER = "https://weldspeak.app";

/** Claims carried by a WeldSpeak access token. */
export interface AccessTokenClaims {
  iss: string;
  /** Clerk user ID. */
  sub: string;
  /** Device ID, so a single device can be revoked without signing out the user. */
  did: string;
  /**
   * The user's org memberships at mint time.
   *
   * Carrying these in the token is what lets the desktop app switch orgs from
   * the tray without re-running the device flow: it names an org per request
   * and the Worker checks it against this list. The list is refreshed from
   * Clerk on every token refresh, so it is at most one refresh cycle stale.
   */
  orgs: Array<{ id: string; role: string }>;
  /**
   * Billing entitlement snapshot from Clerk at mint/refresh time.
   * Missing on pre-billing tokens — treat as `"free"`.
   */
  entitlement?: Entitlement;
  iat: number;
  exp: number;
}

const encoder = new TextEncoder();

function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function base64UrlDecode(value: string): Uint8Array {
  const padded = value.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, "="));
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

async function signingKey(env: Env): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    encoder.encode(env.TOKEN_SIGNING_KEY),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  );
}

/** Mint a signed access token for `userId` on `deviceId`. */
export async function mintAccessToken(
  env: Env,
  userId: string,
  deviceId: string,
  orgs: OrgMembership[],
  entitlement: Entitlement = "free",
): Promise<{ token: string; expiresIn: number }> {
  const now = Math.floor(Date.now() / 1000);
  const claims: AccessTokenClaims = {
    iss: TOKEN_ISSUER,
    sub: userId,
    did: deviceId,
    orgs: orgs.map((org) => ({ id: org.orgId, role: org.role })),
    entitlement,
    iat: now,
    exp: now + ACCESS_TOKEN_TTL_SECONDS,
  };

  const header = base64UrlEncode(encoder.encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const payload = base64UrlEncode(encoder.encode(JSON.stringify(claims)));
  const body = `${header}.${payload}`;

  const signature = await crypto.subtle.sign("HMAC", await signingKey(env), encoder.encode(body));

  return {
    token: `${body}.${base64UrlEncode(new Uint8Array(signature))}`,
    expiresIn: ACCESS_TOKEN_TTL_SECONDS,
  };
}

/**
 * Verify an access token and return its claims, or null if it is malformed,
 * unsigned by us, or expired.
 *
 * Signature comparison goes through `crypto.subtle.verify`, which is constant
 * time; never compare signatures with `===`.
 */
export async function verifyAccessToken(
  env: Env,
  token: string,
): Promise<AccessTokenClaims | null> {
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  const [header, payload, signature] = parts as [string, string, string];

  let valid: boolean;
  try {
    valid = await crypto.subtle.verify(
      "HMAC",
      await signingKey(env),
      base64UrlDecode(signature),
      encoder.encode(`${header}.${payload}`),
    );
  } catch {
    return null;
  }
  if (!valid) return null;

  let claims: AccessTokenClaims;
  try {
    claims = JSON.parse(new TextDecoder().decode(base64UrlDecode(payload)));
  } catch {
    return null;
  }

  if (claims.iss !== TOKEN_ISSUER) return null;
  if (typeof claims.exp !== "number" || claims.exp <= Math.floor(Date.now() / 1000)) return null;
  if (typeof claims.sub !== "string" || typeof claims.did !== "string") return null;
  if (!Array.isArray(claims.orgs)) return null;
  if (
    claims.entitlement != null &&
    claims.entitlement !== "free" &&
    claims.entitlement !== "paid" &&
    claims.entitlement !== "weldsuite"
  ) {
    return null;
  }
  if (claims.entitlement == null) {
    claims.entitlement = "free";
  }

  return claims;
}

/** Generate an opaque refresh token. 32 bytes of CSPRNG output. */
export function generateRefreshToken(): string {
  return base64UrlEncode(crypto.getRandomValues(new Uint8Array(32)));
}

/**
 * Hash a refresh token for storage.
 *
 * Refresh tokens are stored hashed so a database leak yields no live
 * credentials. A plain SHA-256 is right here — unlike a password, the token is
 * full-entropy random, so there is nothing to brute-force and no need for a
 * slow KDF on the hot refresh path.
 */
export async function hashRefreshToken(token: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(token));
  return base64UrlEncode(new Uint8Array(digest));
}
