/**
 * Clerk integration: verifying browser session tokens and reading org membership.
 *
 * Clerk only ever talks to the browser (the web dashboard). The desktop client
 * never holds a Clerk token — see ./tokens.ts for what it carries instead.
 */

import { createClerkClient, verifyToken } from "@clerk/backend";
import type { OrgMembership, OrgRole } from "@weldspeak/protocol";
import type { Env } from "../env.js";

/**
 * Org claims as they appear in a Clerk session token.
 *
 * Clerk changed this shape: v1 tokens carry flat `org_id`/`org_role`/`org_slug`,
 * v2 tokens carry a compact `o` object with abbreviated keys. Which one arrives
 * depends on the token version configured in the Clerk dashboard, so both are
 * handled rather than assuming either.
 */
interface ClerkSessionClaims {
  sub: string;
  org_id?: string;
  org_role?: string;
  org_slug?: string;
  o?: { id?: string; rol?: string; slg?: string };
}

export interface VerifiedClerkSession {
  userId: string;
  /** Active org at the time the token was minted, if the user had one selected. */
  activeOrgId: string | null;
  activeOrgRole: OrgRole | null;
}

/**
 * Clerk abbreviates roles in v2 tokens ("admin") but namespaces them in the
 * Backend API and v1 tokens ("org:admin"). Normalize to the namespaced form,
 * which is what authorization checks compare against.
 */
function normalizeRole(role: string | undefined): OrgRole | null {
  if (!role) return null;
  const namespaced = role.startsWith("org:") ? role : `org:${role}`;
  return namespaced === "org:admin" || namespaced === "org:member"
    ? (namespaced as OrgRole)
    : // A custom role the dashboard defines. Treat it as a plain member:
      // failing closed is the safe direction for an unrecognized role.
      "org:member";
}

/**
 * `@clerk/backend` has shipped both shapes:
 *
 *   - the public export, wrapped by `withLegacyReturn`, yields claims or throws;
 *   - the unwrapped helper yields `{ data }` / `{ errors }` and does not throw.
 *
 * Treat either as success if a `sub` is present, otherwise as a rejected session.
 */
function sessionClaimsFrom(result: unknown): ClerkSessionClaims | null {
  if (!result || typeof result !== "object") return null;

  const record = result as { data?: unknown; sub?: unknown };
  const candidates: unknown[] = [record];
  if (record.data && typeof record.data === "object") {
    candidates.push(record.data);
  }

  for (const candidate of candidates) {
    if (
      candidate &&
      typeof candidate === "object" &&
      typeof (candidate as ClerkSessionClaims).sub === "string" &&
      (candidate as ClerkSessionClaims).sub
    ) {
      return candidate as ClerkSessionClaims;
    }
  }

  return null;
}

/**
 * Verify a Clerk session token from the browser.
 *
 * Returns null rather than throwing, so callers reply 401 uniformly instead of
 * distinguishing between an expired, forged and malformed token — a
 * distinction that only helps an attacker.
 */
export async function verifyClerkSession(
  env: Env,
  token: string,
): Promise<VerifiedClerkSession | null> {
  if (!env.CLERK_SECRET_KEY) {
    console.error("CLERK_SECRET_KEY is not set; browser sessions cannot be verified");
    return null;
  }

  let result: unknown;
  try {
    result = await verifyToken(token, {
      secretKey: env.CLERK_SECRET_KEY,
      clockSkewInMs: 10_000,
    });
  } catch (error) {
    console.warn("clerk session rejected", error);
    return null;
  }

  const claims = sessionClaimsFrom(result);
  if (!claims) {
    console.warn("clerk session rejected", result);
    return null;
  }

  const activeOrgId = claims.o?.id ?? claims.org_id ?? null;
  const activeOrgRole = normalizeRole(claims.o?.rol ?? claims.org_role);

  return { userId: claims.sub, activeOrgId, activeOrgRole };
}

/**
 * List every organization `userId` belongs to.
 *
 * Called when minting or refreshing desktop tokens: the memberships are
 * embedded in the access token so the client can switch orgs without another
 * round trip, and re-read on refresh so a removed member loses access within
 * one refresh cycle.
 */
export async function listOrgMemberships(
  env: Env,
  userId: string,
): Promise<OrgMembership[]> {
  const clerk = createClerkClient({ secretKey: env.CLERK_SECRET_KEY });

  const { data } = await clerk.users.getOrganizationMembershipList({
    userId,
    limit: 100,
  });

  return data.map((membership) => ({
    orgId: membership.organization.id,
    name: membership.organization.name,
    slug: membership.organization.slug ?? null,
    role: normalizeRole(membership.role) ?? "org:member",
  }));
}

/** Fetch profile fields for `GET /me`. */
export async function getUserProfile(
  env: Env,
  userId: string,
): Promise<{ email: string | null; displayName: string | null; imageUrl: string | null }> {
  const clerk = createClerkClient({ secretKey: env.CLERK_SECRET_KEY });
  const user = await clerk.users.getUser(userId);

  const primaryEmail = user.emailAddresses.find(
    (address) => address.id === user.primaryEmailAddressId,
  );

  return {
    email: primaryEmail?.emailAddress ?? user.emailAddresses[0]?.emailAddress ?? null,
    displayName: user.fullName ?? user.username ?? null,
    imageUrl: user.imageUrl ?? null,
  };
}
