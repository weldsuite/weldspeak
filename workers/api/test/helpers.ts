/**
 * Shared test helpers.
 *
 * Tests exercise the Worker through real HTTP requests against Miniflare, with
 * Clerk stubbed out. Clerk's own token verification is not under test here;
 * what is under test is everything we do with the identity it hands us —
 * particularly the org scoping, which is where a team product leaks data.
 */

import { env } from "cloudflare:test";
import { mintAccessToken } from "../src/auth/tokens.js";
import type { OrgMembership } from "@weldspeak/protocol";
import worker from "../src/index.js";

export const ADMIN_USER = "user_admin";
export const MEMBER_USER = "user_member";
export const OUTSIDER_USER = "user_outsider";

export const ORG_ACME = "org_acme";
export const ORG_RIVAL = "org_rival";

export const acmeAdmin: OrgMembership[] = [
  { orgId: ORG_ACME, name: "Acme Welding", slug: "acme", role: "org:admin" },
];
export const acmeMember: OrgMembership[] = [
  { orgId: ORG_ACME, name: "Acme Welding", slug: "acme", role: "org:member" },
];
export const rivalAdmin: OrgMembership[] = [
  { orgId: ORG_RIVAL, name: "Rival Fabrication", slug: "rival", role: "org:admin" },
];

/** Mint a desktop access token for a user with the given memberships. */
export async function tokenFor(
  userId: string,
  orgs: OrgMembership[],
  deviceId = "device_test",
): Promise<string> {
  const { token } = await mintAccessToken(env as never, userId, deviceId, orgs);
  return token;
}

export interface CallOptions {
  method?: string;
  token?: string;
  /** Sent as the X-WeldSpeak-Org header. */
  org?: string;
  body?: unknown;
}

/** Issue a request against the Worker and return the response. */
export async function call(path: string, options: CallOptions = {}): Promise<Response> {
  const headers = new Headers();
  if (options.token) headers.set("Authorization", `Bearer ${options.token}`);
  if (options.org) headers.set("X-WeldSpeak-Org", options.org);
  if (options.body !== undefined) headers.set("Content-Type", "application/json");

  const request = new Request(`https://weldspeak.test${path}`, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });

  return worker.fetch(request, env as never, {
    waitUntil: () => {},
    passThroughOnException: () => {},
  } as unknown as ExecutionContext);
}

/** Insert a dictionary term directly, bypassing the API's authorization. */
export async function seedTerm(
  scope: "user" | "org",
  owner: string,
  term: string,
): Promise<string> {
  const id = crypto.randomUUID();
  await env.DB.prepare(
    `INSERT INTO dictionary_terms (id, scope, clerk_user_id, clerk_org_id, term)
     VALUES (?, ?, ?, ?, ?)`,
  )
    .bind(id, scope, scope === "user" ? owner : null, scope === "org" ? owner : null, term)
    .run();
  return id;
}

/** Insert a transcript directly. */
export async function seedTranscript(
  userId: string,
  orgId: string | null,
  text: string,
): Promise<string> {
  const id = crypto.randomUUID();
  await env.DB.prepare(
    `INSERT INTO transcripts (id, clerk_user_id, clerk_org_id, raw, formatted, duration_ms)
     VALUES (?, ?, ?, ?, ?, ?)`,
  )
    .bind(id, userId, orgId, text, text, 1000)
    .run();
  return id;
}

/** Remove all rows between tests so ordering never matters. */
export async function resetDatabase(): Promise<void> {
  await env.DB.batch([
    env.DB.prepare("DELETE FROM dictionary_terms"),
    env.DB.prepare("DELETE FROM transcripts"),
    env.DB.prepare("DELETE FROM usage"),
    env.DB.prepare("DELETE FROM org_settings"),
    env.DB.prepare("DELETE FROM refresh_tokens"),
    env.DB.prepare("DELETE FROM devices"),
  ]);
}
