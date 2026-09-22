/**
 * Refresh token rotation.
 *
 * Every refresh consumes the old token and issues a new pair. That gives us
 * replay detection: if a token that was already exchanged shows up again,
 * either it leaked or a client is buggy, and the safe response to both is to
 * revoke the whole device.
 *
 * This is also the backstop for membership revocation. The Clerk webhook
 * (./webhook.ts) handles it promptly, but webhooks get dropped, so memberships
 * are re-read from Clerk on every refresh. A user removed from an org loses
 * access within one refresh cycle even if no webhook ever arrives.
 */

import { Hono } from "hono";
import type { RefreshRequest, TokenPair } from "@weldspeak/protocol";
import type { AppBindings } from "./middleware.js";
import { listOrgMemberships } from "./clerk.js";
import { resolveEntitlement } from "../billing/entitlements.js";
import {
  generateRefreshToken,
  hashRefreshToken,
  mintAccessToken,
  REFRESH_TOKEN_TTL_SECONDS,
} from "./tokens.js";

interface RefreshRow {
  token_hash: string;
  device_id: string;
  clerk_user_id: string;
  expires_at: string;
  revoked_at: string | null;
  replaced_by: string | null;
}

export const refreshRoutes = new Hono<AppBindings>();

refreshRoutes.post("/refresh", async (c) => {
  const body = await c.req.json<RefreshRequest>().catch(() => null);
  if (!body?.refreshToken) {
    return c.json({ error: "bad_request", message: "refreshToken is required" }, 400);
  }

  const tokenHash = await hashRefreshToken(body.refreshToken);
  const row = await c.env.DB.prepare(
    `SELECT token_hash, device_id, clerk_user_id, expires_at, revoked_at, replaced_by
     FROM refresh_tokens WHERE token_hash = ?`,
  )
    .bind(tokenHash)
    .first<RefreshRow>();

  if (!row) {
    return c.json({ error: "unauthorized", message: "Invalid refresh token" }, 401);
  }

  // Reuse of an already-rotated or revoked token. Treat it as a compromise and
  // revoke every token for the device, forcing a fresh device authorization.
  if (row.replaced_by || row.revoked_at) {
    await c.env.DB.prepare(
      `UPDATE refresh_tokens SET revoked_at = datetime('now')
       WHERE device_id = ? AND revoked_at IS NULL`,
    )
      .bind(row.device_id)
      .run();

    return c.json(
      { error: "unauthorized", message: "Refresh token reuse detected; device signed out" },
      401,
    );
  }

  if (new Date(row.expires_at).getTime() <= Date.now()) {
    return c.json({ error: "unauthorized", message: "Refresh token expired" }, 401);
  }

  // Re-read memberships and billing from Clerk rather than trusting the
  // previous token. This is what makes removal from an org — and plan changes —
  // take effect within one refresh cycle.
  let orgs;
  let entitlement;
  try {
    [orgs, entitlement] = await Promise.all([
      listOrgMemberships(c.env, row.clerk_user_id),
      resolveEntitlement(c.env, row.clerk_user_id),
    ]);
  } catch {
    // Clerk being unreachable must not silently widen access, but it also
    // should not sign every user out during a Clerk outage. Fail the refresh;
    // the client keeps its unexpired access token and retries.
    return c.json(
      { error: "upstream_failed", message: "Could not verify membership; try again" },
      503,
    );
  }

  const { token: accessToken, expiresIn } = await mintAccessToken(
    c.env,
    row.clerk_user_id,
    row.device_id,
    orgs,
    entitlement,
  );

  const nextRefreshToken = generateRefreshToken();
  const nextHash = await hashRefreshToken(nextRefreshToken);
  const expiresAt = new Date(Date.now() + REFRESH_TOKEN_TTL_SECONDS * 1000).toISOString();

  await c.env.DB.batch([
    c.env.DB.prepare(
      `INSERT INTO refresh_tokens (token_hash, device_id, clerk_user_id, expires_at)
       VALUES (?, ?, ?, ?)`,
    ).bind(nextHash, row.device_id, row.clerk_user_id, expiresAt),
    c.env.DB.prepare(`UPDATE refresh_tokens SET replaced_by = ? WHERE token_hash = ?`).bind(
      nextHash,
      tokenHash,
    ),
    c.env.DB.prepare(`UPDATE devices SET last_seen_at = datetime('now') WHERE id = ?`).bind(
      row.device_id,
    ),
  ]);

  const tokens: TokenPair = { accessToken, refreshToken: nextRefreshToken, expiresIn };
  return c.json(tokens);
});
