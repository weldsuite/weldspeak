/**
 * Organization isolation.
 *
 * These are the tests that matter most for a team product. Everything else
 * degrades a feature when it breaks; this leaks one customer's data to another.
 * They assert against the API directly rather than the UI, because hiding a
 * control in the dashboard is not access control.
 */

import { beforeEach, describe, expect, it } from "vitest";
import {
  ADMIN_USER,
  MEMBER_USER,
  ORG_ACME,
  ORG_RIVAL,
  OUTSIDER_USER,
  acmeAdmin,
  acmeMember,
  call,
  rivalAdmin,
  resetDatabase,
  seedTerm,
  seedTranscript,
  tokenFor,
} from "./helpers.js";

beforeEach(resetDatabase);

describe("shared dictionary", () => {
  it("shows a member their own terms and the org glossary, and nothing else", async () => {
    await seedTerm("user", MEMBER_USER, "my private term");
    await seedTerm("org", ORG_ACME, "Inconel 625");
    await seedTerm("org", ORG_RIVAL, "rival secret alloy");
    await seedTerm("user", OUTSIDER_USER, "someone else's term");

    const response = await call("/api/dictionary", {
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    expect(response.status).toBe(200);
    const { terms } = await response.json<{ terms: Array<{ term: string }> }>();
    const names = terms.map((t) => t.term).sort();

    expect(names).toEqual(["Inconel 625", "my private term"]);
  });

  it("refuses to read another organization's glossary even for its own admin", async () => {
    await seedTerm("org", ORG_RIVAL, "rival secret alloy");

    // An Acme admin naming Rival's org ID: they are not a member, so the
    // request is rejected outright rather than silently scoped to nothing.
    const response = await call("/api/dictionary", {
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      org: ORG_RIVAL,
    });

    expect(response.status).toBe(403);
    expect(await response.json()).toMatchObject({ error: "org_forbidden" });
  });

  it("lets an admin add a shared term", async () => {
    const response = await call("/api/dictionary", {
      method: "POST",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      org: ORG_ACME,
      body: { scope: "org", term: "Inconel 625", soundsLike: "in-co-nel six twenty five" },
    });

    expect(response.status).toBe(201);
    expect(await response.json()).toMatchObject({ scope: "org", term: "Inconel 625" });
  });

  it("refuses a shared-term write from a plain member", async () => {
    const response = await call("/api/dictionary", {
      method: "POST",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
      body: { scope: "org", term: "sneaky term" },
    });

    expect(response.status).toBe(403);

    const { results } = await (
      await import("cloudflare:test")
    ).env.DB.prepare("SELECT term FROM dictionary_terms WHERE scope = 'org'").all();
    expect(results).toHaveLength(0);
  });

  it("still lets a plain member manage their own personal terms", async () => {
    const response = await call("/api/dictionary", {
      method: "POST",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
      body: { scope: "user", term: "my own term" },
    });

    expect(response.status).toBe(201);
  });

  it("refuses to delete a term belonging to another organization", async () => {
    const rivalTermId = await seedTerm("org", ORG_RIVAL, "rival secret alloy");

    // Acme's admin knows the row ID but has no membership in Rival. Deleting by
    // ID alone would destroy it; the owner is part of the WHERE clause.
    const response = await call(`/api/dictionary/${rivalTermId}`, {
      method: "DELETE",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      org: ORG_ACME,
    });

    expect(response.status).toBe(404);

    const survivor = await (
      await import("cloudflare:test")
    ).env.DB.prepare("SELECT id FROM dictionary_terms WHERE id = ?").bind(rivalTermId).first();
    expect(survivor).not.toBeNull();
  });

  it("refuses to delete another user's personal term", async () => {
    const otherId = await seedTerm("user", OUTSIDER_USER, "someone else's term");

    const response = await call(`/api/dictionary/${otherId}`, {
      method: "DELETE",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    expect(response.status).toBe(404);
  });

  it("refuses a member's attempt to delete a shared term", async () => {
    const sharedId = await seedTerm("org", ORG_ACME, "Inconel 625");

    const response = await call(`/api/dictionary/${sharedId}`, {
      method: "DELETE",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    expect(response.status).toBe(404);
  });
});

describe("transcripts", () => {
  it("returns only the caller's own dictations", async () => {
    await seedTranscript(MEMBER_USER, ORG_ACME, "my dictation");
    await seedTranscript(OUTSIDER_USER, ORG_ACME, "a colleague's dictation");

    const response = await call("/api/transcripts", {
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    const { transcripts } = await response.json<{ transcripts: Array<{ raw: string }> }>();
    expect(transcripts.map((t) => t.raw)).toEqual(["my dictation"]);
  });

  it("does not let an org admin read a colleague's dictations", async () => {
    await seedTranscript(MEMBER_USER, ORG_ACME, "something private");

    // Admins set policy and see aggregate usage. Reading what colleagues
    // dictated is surveillance, and there is deliberately no route for it.
    const response = await call("/api/transcripts", {
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      org: ORG_ACME,
    });

    const { transcripts } = await response.json<{ transcripts: unknown[] }>();
    expect(transcripts).toHaveLength(0);
  });

  it("refuses to delete another user's transcript", async () => {
    const id = await seedTranscript(OUTSIDER_USER, ORG_ACME, "not yours");

    const response = await call(`/api/transcripts/${id}`, {
      method: "DELETE",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    expect(response.status).toBe(404);
  });
});

describe("org settings", () => {
  it("lets any member read the policy that governs their own dictations", async () => {
    const response = await call("/api/org/settings", {
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ retainTranscripts: true });
  });

  it("refuses a policy change from a plain member", async () => {
    const response = await call("/api/org/settings", {
      method: "PATCH",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
      body: { retainTranscripts: false },
    });

    expect(response.status).toBe(403);
  });

  it("lets an admin turn transcript retention off", async () => {
    const response = await call("/api/org/settings", {
      method: "PATCH",
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      org: ORG_ACME,
      body: { retainTranscripts: false, monthlyMinuteCap: 500 },
    });

    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({
      retainTranscripts: false,
      monthlyMinuteCap: 500,
    });
  });

  it("does not let an admin of one org change another org's policy", async () => {
    const response = await call("/api/org/settings", {
      method: "PATCH",
      token: await tokenFor(ADMIN_USER, rivalAdmin),
      org: ORG_ACME,
      body: { retainTranscripts: false },
    });

    expect(response.status).toBe(403);
  });
});

describe("usage", () => {
  beforeEach(async () => {
    const { env } = await import("cloudflare:test");
    const day = new Date().toISOString().slice(0, 10);
    await env.DB.batch([
      env.DB.prepare(
        "INSERT INTO usage (clerk_org_id, clerk_user_id, day, audio_seconds) VALUES (?, ?, ?, ?)",
      ).bind(ORG_ACME, ADMIN_USER, day, 120),
      env.DB.prepare(
        "INSERT INTO usage (clerk_org_id, clerk_user_id, day, audio_seconds) VALUES (?, ?, ?, ?)",
      ).bind(ORG_ACME, MEMBER_USER, day, 60),
    ]);
  });

  it("gives an admin the per-member breakdown", async () => {
    const response = await call("/api/org/usage", {
      token: await tokenFor(ADMIN_USER, acmeAdmin),
      org: ORG_ACME,
    });

    const body = await response.json<{ audioSeconds: number; byUser: unknown[] }>();
    expect(body.audioSeconds).toBe(180);
    expect(body.byUser).toHaveLength(2);
  });

  it("shows a plain member the org total but only their own line", async () => {
    const response = await call("/api/org/usage", {
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
    });

    const body = await response.json<{
      audioSeconds: number;
      byUser: Array<{ userId: string }>;
    }>();

    expect(body.audioSeconds).toBe(180);
    expect(body.byUser).toEqual([{ userId: MEMBER_USER, audioSeconds: 60 }]);
  });
});
