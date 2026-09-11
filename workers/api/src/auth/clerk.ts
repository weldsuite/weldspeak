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
  let claims: ClerkSessionClaims;
  try {
    claims = (await verifyToken(token, {
      secretKey: env.CLERK_SECRET_KEY,
      // The live publishable key names clerk.weldsuite.org as the Frontend API,
      // so JWKS is fetched from there rather than from Clerk's default host.
      publishableKey: env.CLERK_PUBLISHABLE_KEY,
    })) as unknown as ClerkSessionClaims;
  } catch {
    return null;
  }

  if (!claims.sub) return null;

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
