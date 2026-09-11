/**
 * Authentication and organization-scoping middleware.
 *
 * Two kinds of caller reach this Worker:
 *
 *   - the **browser** (web dashboard), carrying a Clerk session token;
 *   - the **desktop app**, carrying a WeldSpeak access token we minted.
 *
 * Both resolve to the same `AuthContext`, so route handlers below this layer
 * never need to know which one they are serving.
 *
 * The rule every handler depends on: `activeOrg` is only ever set to an org the
 * caller is provably a member of. A client-supplied org ID is a *request*, not
 * a fact, and is checked against the caller's memberships before it is honoured.
 */

import type { Context, MiddlewareHandler, Next } from "hono";
import type { OrgRole } from "@weldspeak/protocol";
import type { Env } from "../env.js";
import { verifyClerkSession } from "./clerk.js";
import { verifyAccessToken } from "./tokens.js";

export interface AuthContext {
  userId: string;
  /** Set for desktop callers; null for browser sessions. */
  deviceId: string | null;
  /** Every org the caller belongs to. */
  orgs: Array<{ id: string; role: OrgRole }>;
  /** The org this request operates in, already verified. Null for personal scope. */
  activeOrg: { id: string; role: OrgRole } | null;
}

export interface AppBindings {
  Bindings: Env;
  Variables: { auth: AuthContext };
}

export type AppContext = Context<AppBindings>;

/** Header the desktop client uses to name its active organization. */
export const ORG_HEADER = "X-WeldSpeak-Org";

function bearerToken(c: AppContext): string | null {
  const header = c.req.header("Authorization");
  if (!header?.startsWith("Bearer ")) return null;
  const token = header.slice("Bearer ".length).trim();
  return token.length > 0 ? token : null;
}

/**
 * Resolve the requested org against the caller's memberships.
 *
 * Returns `undefined` when the caller asked for an org they are not in — the
 * caller distinguishes that (403) from "no org requested" (personal scope).
 */
function resolveActiveOrg(
  orgs: Array<{ id: string; role: OrgRole }>,
  requestedOrgId: string | null,
): { id: string; role: OrgRole } | null | undefined {
  if (!requestedOrgId) return null;
  const membership = orgs.find((org) => org.id === requestedOrgId);
  return membership ? { id: membership.id, role: membership.role } : undefined;
}

/**
 * Accept either a desktop access token or a Clerk browser session.
 *
 * Desktop tokens are tried first because they are verified locally with no
 * network call; a Clerk token requires a JWKS lookup.
 */
export function requireAuth(): MiddlewareHandler<AppBindings> {
  return async (c: AppContext, next: Next) => {
    const token = bearerToken(c);
    if (!token) {
      return c.json({ error: "unauthorized", message: "Missing bearer token" }, 401);
    }

    const desktopClaims = await verifyAccessToken(c.env, token);
    if (desktopClaims) {
      const orgs = desktopClaims.orgs.map((org) => ({
        id: org.id,
        role: (org.role === "org:admin" ? "org:admin" : "org:member") as OrgRole,
      }));

      const activeOrg = resolveActiveOrg(orgs, c.req.header(ORG_HEADER) ?? null);
      if (activeOrg === undefined) {
        return c.json(
          { error: "org_forbidden", message: "Not a member of the requested organization" },
          403,
        );
      }

      c.set("auth", {
        userId: desktopClaims.sub,
        deviceId: desktopClaims.did,
        orgs,
        activeOrg,
      });
      return next();
    }

    if (!c.env.CLERK_SECRET_KEY) {
      console.error("CLERK_SECRET_KEY is not set; browser sessions cannot be verified");
      return c.json(
        { error: "misconfigured", message: "Sign-in is not configured on the server." },
        503,
      );
    }

    const clerkSession = await verifyClerkSession(c.env, token);
    if (!clerkSession) {
      return c.json({ error: "unauthorized", message: "Invalid or expired token" }, 401);
    }

    // A Clerk token names the org the browser has selected, and Clerk only
    // issues that claim for orgs the user actually belongs to — so it needs no
    // further membership check. The desktop path above is the one where the
    // org arrives from the client and must be validated.
    const activeOrg =
      clerkSession.activeOrgId && clerkSession.activeOrgRole
        ? { id: clerkSession.activeOrgId, role: clerkSession.activeOrgRole }
        : null;

    c.set("auth", {
      userId: clerkSession.userId,
      deviceId: null,
      orgs: activeOrg ? [activeOrg] : [],
      activeOrg,
    });
    return next();
  };
}

/**
 * Require an active organization on the request.
 *
 * Use on routes that are meaningless in personal scope, such as team
 * management or org usage.
 */
export function requireOrg(): MiddlewareHandler<AppBindings> {
  return async (c: AppContext, next: Next) => {
    if (!c.get("auth").activeOrg) {
      return c.json(
        { error: "bad_request", message: "This endpoint requires an active organization" },
        400,
      );
    }
    return next();
  };
}

/**
 * Require org admin.
 *
 * The dashboard also hides admin surfaces with Clerk's `<Show>`, but that only
 * removes markup from the page — this is the check that actually enforces it.
 */
export function requireOrgAdmin(): MiddlewareHandler<AppBindings> {
  return async (c: AppContext, next: Next) => {
    const { activeOrg } = c.get("auth");
    if (!activeOrg) {
      return c.json(
        { error: "bad_request", message: "This endpoint requires an active organization" },
        400,
      );
    }
    if (activeOrg.role !== "org:admin") {
      return c.json(
        { error: "org_forbidden", message: "This action requires an organization admin" },
        403,
      );
    }
    return next();
  };
}
