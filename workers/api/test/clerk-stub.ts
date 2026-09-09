/**
 * A stand-in for Clerk's Backend API.
 *
 * Stubbing at the network boundary rather than mocking our own module keeps
 * `listOrgMemberships` in the test path, so role normalization and response
 * parsing are actually exercised. Mocking the function away would skip exactly
 * the code most likely to break when Clerk changes a payload shape.
 *
 * Timing matters here: `@clerk/backend` captures `fetch` at module load
 * (`var globalFetch = fetch.bind(globalThis)`), so replacing the global later
 * has no effect. `installClerkInterceptor()` therefore runs from test/setup.ts,
 * before any test file — and with it, `@clerk/backend` — is imported. Tests
 * then swap the *handler* rather than the global.
 */

import type { OrgMembership } from "@weldspeak/protocol";

const CLERK_API = "https://api.clerk.com";

type ClerkHandler = (url: string) => Response | null;

/** Current handler, swapped per test by `stubClerk`. */
let handler: ClerkHandler = () => null;

let installed = false;

/**
 * Route Clerk API calls through a swappable handler.
 *
 * Must be called before anything imports `@clerk/backend`.
 */
export function installClerkInterceptor(): void {
  if (installed) return;
  installed = true;

  const realFetch = globalThis.fetch;

  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;

    if (url.startsWith(CLERK_API)) {
      return (
        handler(url) ?? new Response(`unstubbed Clerk endpoint: ${url}`, { status: 501 })
      );
    }
    return realFetch(input as RequestInfo, init);
  }) as typeof fetch;
}

/** Shape one membership the way Clerk's API returns it. */
function membershipPayload(membership: OrgMembership, userId: string) {
  return {
    id: `orgmem_${membership.orgId}`,
    // Clerk returns the namespaced form here, unlike the abbreviated form used
    // in v2 session tokens — the asymmetry our role normalization exists for.
    role: membership.role,
    permissions: [],
    public_metadata: {},
    private_metadata: {},
    created_at: 0,
    updated_at: 0,
    organization: {
      id: membership.orgId,
      name: membership.name,
      slug: membership.slug,
      members_count: 1,
      max_allowed_memberships: 5,
      admin_delete_enabled: true,
      public_metadata: {},
      private_metadata: {},
      created_at: 0,
      updated_at: 0,
    },
    public_user_data: { user_id: userId },
  };
}

export interface ClerkStub {
  /** Change what Clerk reports for a user, e.g. after removing them from an org. */
  setMemberships(userId: string, next: OrgMembership[]): void;
  /** Make every Clerk call fail, to exercise the outage path. */
  breakClerk(): void;
}

/** Serve canned Clerk responses for the duration of a test. */
export function stubClerk(initial: Record<string, OrgMembership[]>): ClerkStub {
  const memberships = new Map(Object.entries(initial));
  let broken = false;

  handler = (url) => {
    if (broken) return new Response("Clerk is down", { status: 503 });

    const orgMemberships = url.match(/\/users\/([^/]+)\/organization_memberships/);
    if (orgMemberships) {
      const userId = decodeURIComponent(orgMemberships[1]!);
      const data = (memberships.get(userId) ?? []).map((m) => membershipPayload(m, userId));
      return Response.json({ data, total_count: data.length });
    }

    const user = url.match(/\/users\/([^/?]+)(?:\?|$)/);
    if (user) {
      const userId = decodeURIComponent(user[1]!);
      return Response.json({
        id: userId,
        first_name: "Test",
        last_name: "User",
        username: null,
        image_url: "https://img.clerk.test/avatar.png",
        primary_email_address_id: "idn_1",
        email_addresses: [
          { id: "idn_1", email_address: `${userId}@weldspeak.test`, verification: null },
        ],
        public_metadata: {},
        private_metadata: {},
        unsafe_metadata: {},
        created_at: 0,
        updated_at: 0,
      });
    }

    return null;
  };

  return {
    setMemberships(userId, next) {
      memberships.set(userId, next);
    },
    breakClerk() {
      broken = true;
    },
  };
}

/** Restore the do-nothing handler between tests. */
export function resetClerkStub(): void {
  handler = () => null;
}
