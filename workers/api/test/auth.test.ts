/**
 * Token minting, verification and the device authorization grant.
 *
 * Clerk's own verification is not under test — that is Clerk's job. What is
 * under test is everything we build on top: that our tokens cannot be forged
 * or replayed, that org membership in a token is honoured, and that the device
 * grant hands a desktop install exactly one set of credentials.
 */

import { env } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DevicePollResponse, DeviceStartResponse } from "@weldspeak/protocol";
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
import {
  generateRefreshToken,
  hashRefreshToken,
  mintAccessToken,
  verifyAccessToken,
} from "../src/auth/tokens.js";

import { resetClerkStub, stubClerk } from "./clerk-stub.js";

// The device grant reads memberships from Clerk when issuing tokens. Stub the
// HTTP boundary rather than our own module, so the real parsing and role
// normalization stay in the test path.
beforeEach(async () => {
  await resetDatabase();
  stubClerk({ [ADMIN_USER]: acmeAdmin, [MEMBER_USER]: acmeMember });
});

afterEach(resetClerkStub);

describe("access tokens", () => {
  it("round-trips its claims", async () => {
    const { token } = await mintAccessToken(env, ADMIN_USER, "device_1", acmeAdmin);
    const claims = await verifyAccessToken(env, token);

    expect(claims?.sub).toBe(ADMIN_USER);
    expect(claims?.did).toBe("device_1");
    expect(claims?.orgs).toEqual([{ id: ORG_ACME, role: "org:admin" }]);
  });

  it("rejects a token whose payload has been edited", async () => {
    const { token } = await mintAccessToken(env, MEMBER_USER, "device_1", acmeMember);
    const [header, payload, signature] = token.split(".") as [string, string, string];

    // Promote the member to admin in the payload, keeping the original
    // signature. This is the attack the signature exists to stop.
    const claims = JSON.parse(atob(payload.replace(/-/g, "+").replace(/_/g, "/")));
    claims.orgs = [{ id: ORG_ACME, role: "org:admin" }];
    const forged = btoa(JSON.stringify(claims))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");

    expect(await verifyAccessToken(env, `${header}.${forged}.${signature}`)).toBeNull();
  });

  it("rejects an expired token", async () => {
    vi.useFakeTimers();
    try {
      const { token } = await mintAccessToken(env, ADMIN_USER, "device_1", acmeAdmin);
      vi.setSystemTime(Date.now() + 2 * 60 * 60 * 1000); // past the 1-hour TTL
      expect(await verifyAccessToken(env, token)).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("rejects malformed tokens", async () => {
    for (const bad of ["", "not-a-jwt", "a.b", "a.b.c.d"]) {
      expect(await verifyAccessToken(env, bad)).toBeNull();
    }
  });

  it("stores refresh tokens hashed, never in the clear", async () => {
    const token = generateRefreshToken();
    const hash = await hashRefreshToken(token);

    expect(hash).not.toBe(token);
    // Same input, same hash — the lookup on refresh depends on this.
    expect(await hashRefreshToken(token)).toBe(hash);
  });
});

describe("request authorization", () => {
  it("rejects a request with no token", async () => {
    expect((await call("/api/dictionary")).status).toBe(401);
  });

  it("rejects a garbage bearer token", async () => {
    expect((await call("/api/dictionary", { token: "nonsense" })).status).toBe(401);
  });

  it("treats a request with no org header as personal scope", async () => {
    const response = await call("/api/dictionary", {
      token: await tokenFor(MEMBER_USER, acmeMember),
    });

    expect(response.status).toBe(200);
  });

  it("rejects an org the token does not vouch for", async () => {
    const response = await call("/api/dictionary", {
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: "org_never_heard_of",
    });

    expect(response.status).toBe(403);
  });
});

describe("device authorization grant", () => {
  async function startGrant(): Promise<DeviceStartResponse> {
    const response = await call("/auth/device/start", {
      method: "POST",
      body: { platform: "macos", label: "Workshop MacBook" },
    });
    expect(response.status).toBe(200);
    return response.json<DeviceStartResponse>();
  }

  it("issues a device code and a human-readable user code", async () => {
    const grant = await startGrant();

    expect(grant.deviceCode).toHaveLength(64);
    expect(grant.userCode).toMatch(/^[A-Z2-9]{4}-[A-Z2-9]{4}$/);
    expect(grant.verifyUrl).toContain(encodeURIComponent(grant.userCode));
  });

  it("omits characters that look alike from the user code", async () => {
    // The user reads this off one screen and confirms it on another, so 0/O and
    // 1/I/L would cause real failures.
    for (let i = 0; i < 20; i++) {
      const { userCode } = await startGrant();
      expect(userCode).not.toMatch(/[01OIL]/);
    }
  });

  it("reports pending until the browser approves", async () => {
    const grant = await startGrant();

    const response = await call("/auth/device/poll", {
      method: "POST",
      body: { deviceCode: grant.deviceCode },
    });

    expect(await response.json<DevicePollResponse>()).toEqual({
      status: "authorization_pending",
    });
  });

  it("issues tokens once approved, and registers the device", async () => {
    const grant = await startGrant();

    const approval = await call("/auth/device/approve", {
      method: "POST",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      body: { userCode: grant.userCode },
    });
    expect(approval.status).toBe(200);

    // The poll interval rejects a second poll within two seconds of the first,
    // so this test polls only once — after approval.
    const poll = await call("/auth/device/poll", {
      method: "POST",
      body: { deviceCode: grant.deviceCode },
    });

    const result = await poll.json<DevicePollResponse>();
    expect(result.status).toBe("approved");
    if (result.status !== "approved") throw new Error("unreachable");

    expect(result.tokens.accessToken).toBeTruthy();
    expect(result.tokens.refreshToken).toBeTruthy();

    const claims = await verifyAccessToken(env, result.tokens.accessToken);
    expect(claims?.sub).toBe(ADMIN_USER);

    const device = await env.DB.prepare(
      "SELECT platform, label FROM devices WHERE clerk_user_id = ?",
    )
      .bind(ADMIN_USER)
      .first();
    expect(device).toMatchObject({ platform: "macos", label: "Workshop MacBook" });
  });

  it("consumes the grant, so a leaked device code cannot be replayed", async () => {
    const grant = await startGrant();

    await call("/auth/device/approve", {
      method: "POST",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      body: { userCode: grant.userCode },
    });
    await call("/auth/device/poll", { method: "POST", body: { deviceCode: grant.deviceCode } });

    // Second exchange of the same code: the grant is gone.
    const replay = await call("/auth/device/poll", {
      method: "POST",
      body: { deviceCode: grant.deviceCode },
    });

    expect(await replay.json<DevicePollResponse>()).toEqual({ status: "expired" });
  });

  it("refuses to approve an unknown code", async () => {
    const response = await call("/auth/device/approve", {
      method: "POST",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      body: { userCode: "ZZZZ-9999" },
    });

    expect(response.status).toBe(404);
  });

  it("refuses to approve without authentication", async () => {
    const grant = await startGrant();

    const response = await call("/auth/device/approve", {
      method: "POST",
      body: { userCode: grant.userCode },
    });

    expect(response.status).toBe(401);
  });

  it("reports denial to the waiting device", async () => {
    const grant = await startGrant();

    await call("/auth/device/deny", {
      method: "POST",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      body: { userCode: grant.userCode },
    });

    const poll = await call("/auth/device/poll", {
      method: "POST",
      body: { deviceCode: grant.deviceCode },
    });

    expect(await poll.json<DevicePollResponse>()).toEqual({ status: "denied" });
  });
});
