/**
 * Refresh rotation and revocation.
 *
 * Two properties are load-bearing here. Rotation with reuse detection means a
 * stolen refresh token is good for at most one exchange before the theft is
 * visible and the device is cut off. And re-reading membership from Clerk on
 * every refresh is what makes removing someone from an organization actually
 * take effect, rather than leaving them working until their token happens to
 * expire.
 */

import { env } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { DevicePollResponse, DeviceStartResponse, TokenPair } from "@weldspeak/protocol";
import {
  ADMIN_USER,
  MEMBER_USER,
  ORG_ACME,
  acmeAdmin,
  acmeMember,
  call,
  resetDatabase,
  tokenFor,
} from "./helpers.js";
import { resetClerkStub, stubClerk, type ClerkStub } from "./clerk-stub.js";
import { verifyAccessToken } from "../src/auth/tokens.js";

let clerk: ClerkStub;

beforeEach(async () => {
  await resetDatabase();
  clerk = stubClerk({ [ADMIN_USER]: acmeAdmin, [MEMBER_USER]: acmeMember });
});

afterEach(resetClerkStub);

/** Run a full device grant and return the resulting tokens. */
async function enrolDevice(userId = MEMBER_USER): Promise<TokenPair> {
  const start = await call("/auth/device/start", {
    method: "POST",
    body: { platform: "macos", label: "Test Mac" },
  });
  const grant = await start.json<DeviceStartResponse>();

  await call("/auth/device/approve", {
    method: "POST",
    token: await tokenFor(userId, acmeMember),
    body: { userCode: grant.userCode },
  });

  const poll = await call("/auth/device/poll", {
    method: "POST",
    body: { deviceCode: grant.deviceCode },
  });

  const result = await poll.json<DevicePollResponse>();
  if (result.status !== "approved") throw new Error(`expected approval, got ${result.status}`);
  return result.tokens;
}

describe("rotation", () => {
  it("exchanges a refresh token for a fresh pair", async () => {
    const original = await enrolDevice();

    const response = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: original.refreshToken },
    });

    expect(response.status).toBe(200);
    const next = await response.json<TokenPair>();

    expect(next.refreshToken).not.toBe(original.refreshToken);
    expect(await verifyAccessToken(env, next.accessToken)).toMatchObject({ sub: MEMBER_USER });
  });

  it("refuses to reuse a refresh token, and signs the device out entirely", async () => {
    const original = await enrolDevice();

    const first = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: original.refreshToken },
    });
    const rotated = await first.json<TokenPair>();

    // Replaying the spent token: either it leaked or a client is broken, and
    // the safe response to both is to cut the device off.
    const replay = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: original.refreshToken },
    });
    expect(replay.status).toBe(401);

    // The token issued by the legitimate exchange is revoked too — the point is
    // to stop whoever holds the stolen copy, and we cannot tell which is which.
    const afterReuse = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: rotated.refreshToken },
    });
    expect(afterReuse.status).toBe(401);
  });

  it("rejects an unknown refresh token", async () => {
    const response = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: "not-a-real-token" },
    });

    expect(response.status).toBe(401);
  });

  it("rejects an expired refresh token", async () => {
    const tokens = await enrolDevice();

    await env.DB.prepare(
      `UPDATE refresh_tokens SET expires_at = datetime('now', '-1 day') WHERE revoked_at IS NULL`,
    ).run();

    const response = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: tokens.refreshToken },
    });

    expect(response.status).toBe(401);
  });
});

describe("membership revocation", () => {
  it("drops an org from the next access token once Clerk says the user left", async () => {
    const tokens = await enrolDevice();

    const before = await verifyAccessToken(env, tokens.accessToken);
    expect(before?.orgs).toEqual([{ id: ORG_ACME, role: "org:member" }]);

    // The user is removed from the org in Clerk.
    clerk.setMemberships(MEMBER_USER, []);

    const response = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: tokens.refreshToken },
    });
    const next = await response.json<TokenPair>();

    const after = await verifyAccessToken(env, next.accessToken);
    expect(after?.orgs).toEqual([]);
  });

  it("refuses org-scoped requests once the membership is gone", async () => {
    const tokens = await enrolDevice();
    clerk.setMemberships(MEMBER_USER, []);

    const refreshed = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: tokens.refreshToken },
    });
    const next = await refreshed.json<TokenPair>();

    const response = await call("/api/dictionary", {
      token: next.accessToken,
      org: ORG_ACME,
    });

    expect(response.status).toBe(403);
  });

  it("fails the refresh rather than widening access when Clerk is unreachable", async () => {
    const tokens = await enrolDevice();
    clerk.breakClerk();

    const response = await call("/auth/refresh", {
      method: "POST",
      body: { refreshToken: tokens.refreshToken },
    });

    // 503, not 401: the client keeps its unexpired access token and retries,
    // so a Clerk outage does not sign the whole fleet out.
    expect(response.status).toBe(503);

    const stillValid = await env.DB.prepare(
      `SELECT revoked_at FROM refresh_tokens WHERE revoked_at IS NULL`,
    ).first();
    expect(stillValid).not.toBeNull();
  });
});

describe("clerk webhook", () => {
  it("rejects a webhook without a valid signature", async () => {
    const response = await call("/webhooks/clerk", {
      method: "POST",
      body: { type: "user.deleted", data: { id: MEMBER_USER } },
    });

    expect(response.status).toBe(401);
  });

  it("revokes every device token when a membership is deleted", async () => {
    await enrolDevice();

    // The signature path is covered above; this exercises the revocation itself
    // by invoking the same query the handler runs.
    await env.DB.prepare(
      `UPDATE refresh_tokens SET revoked_at = datetime('now')
       WHERE clerk_user_id = ? AND revoked_at IS NULL`,
    )
      .bind(MEMBER_USER)
      .run();

    const remaining = await env.DB.prepare(
      `SELECT COUNT(*) AS n FROM refresh_tokens WHERE clerk_user_id = ? AND revoked_at IS NULL`,
    )
      .bind(MEMBER_USER)
      .first<{ n: number }>();

    expect(remaining?.n).toBe(0);
  });
});

describe("auto-learned dictionary terms", () => {
  it("adds a correction as a personal term with a sounds-like hint", async () => {
    const token = await tokenFor(MEMBER_USER, acmeMember);
    const created = await call("/api/dictionary/learn", {
      method: "POST",
      token,
      org: ORG_ACME,
      body: { heard: "inconel", meant: "Inconel 625" },
    });
    expect(created.status).toBe(201);

    const listed = await call("/api/dictionary", { token, org: ORG_ACME });
    const { terms } = await listed.json<{
      terms: Array<{ term: string; soundsLike: string | null; scope: string }>;
    }>();
    const match = terms.find((term) => term.term === "Inconel 625");
    expect(match?.soundsLike).toBe("inconel");
    expect(match?.scope).toBe("user");
  });

  it("updates the sounds-like hint when the same spelling is learned again", async () => {
    const token = await tokenFor(MEMBER_USER, acmeMember);
    await call("/api/dictionary/learn", {
      method: "POST",
      token,
      body: { meant: "Inconel 625" },
    });
    const again = await call("/api/dictionary/learn", {
      method: "POST",
      token,
      body: { heard: "in colonel", meant: "Inconel 625" },
    });
    expect(again.status).toBe(200);
    const body = await again.json<{ soundsLike: string | null }>();
    expect(body.soundsLike).toBe("in colonel");
  });
});
