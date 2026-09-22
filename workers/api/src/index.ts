/**
 * WeldSpeak API Worker.
 *
 * Serves three things from one deployment: the JSON API, the dictation
 * WebSocket, and the static web dashboard. Production hostname is
 * `weldspeak.weldsuite.org`, with `api.weldspeak.com` as a second name for the
 * same Worker (marketing lives on Vercel at `weldspeak.com`).
 * Co-hosting the dashboard on the API host keeps the browser same-origin, so
 * the device-approval path has no CORS preflight.
 */

import { Hono } from "hono";
import { decodeTokenSubprotocol, encodeTokenSubprotocol } from "@weldspeak/protocol";
import type { MeResponse } from "@weldspeak/protocol";
import type { AppBindings } from "./auth/middleware.js";
import { requireAuth } from "./auth/middleware.js";
import { deviceRoutes } from "./auth/device.js";
import { refreshRoutes } from "./auth/refresh.js";
import { webhookRoutes } from "./auth/webhook.js";
import { getUserProfile, listOrgMemberships, nameTokenOrgs } from "./auth/clerk.js";
import { monthlyWordCap } from "./billing/entitlements.js";
import { dictionaryRoutes } from "./routes/dictionary.js";
import { transcriptRoutes } from "./routes/transcripts.js";
import { orgRoutes } from "./routes/org.js";
import { verifyAccessToken } from "./auth/tokens.js";
import type { SessionIdentity } from "./session-do.js";

export { DictationSession } from "./session-do.js";

const app = new Hono<AppBindings>();

app.get("/health", (c) => c.json({ ok: true }));

app.route("/auth/device", deviceRoutes);
app.route("/auth", refreshRoutes);
app.route("/webhooks", webhookRoutes);
app.route("/api/dictionary", dictionaryRoutes);
app.route("/api/transcripts", transcriptRoutes);
app.route("/api/org", orgRoutes);

app.get("/api/me", requireAuth(), async (c) => {
  const { userId, orgs: tokenOrgs, deviceId, entitlement } = c.get("auth");

  const profile = await getUserProfile(c.env, userId);

  // A browser caller's token names only its active org, so memberships come
  // from Clerk. A desktop token already carries the authoritative list, but
  // only IDs — the app showed "org_3J6A…" as the organization name. Names are
  // looked up for display; membership still comes from the token alone.
  const orgs = deviceId
    ? await nameTokenOrgs(c.env, userId, tokenOrgs)
    : await listOrgMemberships(c.env, userId);

  return c.json({
    userId,
    ...profile,
    orgs,
    entitlement,
    monthlyWordCap: monthlyWordCap(entitlement),
  } satisfies MeResponse);
});

/**
 * Dictation stream.
 *
 * Authenticated here rather than inside the Durable Object so an unauthorised
 * connection never allocates one. The verified identity is handed to the DO as
 * a header — the DO trusts it because only this Worker can reach it.
 */
app.get("/v1/stream", async (c) => {
  if (c.req.header("Upgrade") !== "websocket") {
    return c.json({ error: "bad_request", message: "Expected a WebSocket upgrade" }, 426);
  }

  const offered = c.req.header("Sec-WebSocket-Protocol") ?? null;
  const token = decodeTokenSubprotocol(offered);
  if (!token) {
    return c.json({ error: "unauthorized", message: "Missing token subprotocol" }, 401);
  }

  const claims = await verifyAccessToken(c.env, token);
  if (!claims) {
    return c.json({ error: "unauthorized", message: "Invalid or expired token" }, 401);
  }

  // The org arrives from the client, so it is checked against the memberships
  // in the token before it is used to scope anything.
  const requestedOrgId = c.req.query("org") ?? null;
  if (requestedOrgId && !claims.orgs.some((org) => org.id === requestedOrgId)) {
    return c.json(
      { error: "org_forbidden", message: "Not a member of the requested organization" },
      403,
    );
  }

  const identity: SessionIdentity = {
    userId: claims.sub,
    orgId: requestedOrgId,
    entitlement: claims.entitlement ?? "free",
  };

  // One DO per connection: a dictation is a single utterance with no state to
  // share, so a fresh ID avoids any cross-session interference.
  const stub = c.env.DICTATION.get(c.env.DICTATION.newUniqueId());

  const response = await stub.fetch(
    new Request(c.req.url, {
      headers: {
        Upgrade: "websocket",
        "X-WeldSpeak-Identity": JSON.stringify(identity),
      },
    }),
  );

  // Echo the accepted subprotocol; browsers fail the handshake without it.
  const headers = new Headers(response.headers);
  headers.set("Sec-WebSocket-Protocol", encodeTokenSubprotocol(token));

  return new Response(response.body, {
    status: response.status,
    webSocket: response.webSocket,
    headers,
  });
});

// Anything not matched above is the dashboard SPA.
app.all("*", (c) => c.env.ASSETS.fetch(c.req.raw));

export default app;
