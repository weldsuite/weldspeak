/**
 * Clerk webhook receiver.
 *
 * This is the fast path for revocation: when someone is removed from an org or
 * their account is deleted, Clerk tells us within seconds and we revoke their
 * desktop refresh tokens immediately. The slow path — re-reading memberships
 * on every token refresh (../auth/refresh.ts) — is the backstop for when a
 * webhook is dropped or arrives out of order.
 *
 * Neither path alone is sufficient. The webhook is fast but unreliable; the
 * refresh check is reliable but up to an hour late.
 */

import { Hono } from "hono";
import { Webhook } from "svix";
import type { AppBindings } from "./middleware.js";

/** The Clerk events we act on. Everything else is acknowledged and ignored. */
type ClerkWebhookEvent =
  | { type: "user.deleted"; data: { id: string } }
  | { type: "session.revoked"; data: { user_id: string } }
  | { type: "organizationMembership.deleted"; data: { public_user_data: { user_id: string } } }
  | { type: string; data: Record<string, unknown> };

export const webhookRoutes = new Hono<AppBindings>();

webhookRoutes.post("/clerk", async (c) => {
  const payload = await c.req.text();

  // Svix signs over the raw body, so verification must happen before parsing.
  const headers = {
    "svix-id": c.req.header("svix-id") ?? "",
    "svix-timestamp": c.req.header("svix-timestamp") ?? "",
    "svix-signature": c.req.header("svix-signature") ?? "",
  };

  let event: ClerkWebhookEvent;
  try {
    event = new Webhook(c.env.CLERK_WEBHOOK_SECRET).verify(payload, headers) as ClerkWebhookEvent;
  } catch {
    return c.json({ error: "unauthorized", message: "Invalid webhook signature" }, 401);
  }

  switch (event.type) {
    case "user.deleted":
    case "session.revoked": {
      const data = event.data as { id?: string; user_id?: string };
      const userId = data.user_id ?? data.id;
      if (userId) await revokeUserTokens(c.env.DB, userId);
      break;
    }

    case "organizationMembership.deleted": {
      // Revoke every device for the removed member rather than trying to strip
      // one org from their live tokens. Access tokens are immutable once
      // minted, so signing the device out is the only way to drop the org
      // before the token would have expired on its own.
      const data = event.data as { public_user_data?: { user_id?: string } };
      const userId = data.public_user_data?.user_id;
      if (userId) await revokeUserTokens(c.env.DB, userId);
      break;
    }

    default:
      break;
  }

  return c.json({ received: true });
});

async function revokeUserTokens(db: D1Database, clerkUserId: string): Promise<void> {
  await db
    .prepare(
      `UPDATE refresh_tokens SET revoked_at = datetime('now')
       WHERE clerk_user_id = ? AND revoked_at IS NULL`,
    )
    .bind(clerkUserId)
    .run();
}
