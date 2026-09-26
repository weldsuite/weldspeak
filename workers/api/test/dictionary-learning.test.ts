/**
 * What the dictionary learns, and what it refuses to.
 *
 * Older desktop builds added words picked out of each dictation's own output
 * ("Maybe", "Honestly", misheard words) to the dictionary, and every one was
 * boosted on the next dictation. These tests pin down that only corrections
 * are learned now, and that the harvested rows already stored stay out of the
 * way without being deleted.
 */

import { env } from "cloudflare:test";
import { beforeEach, describe, expect, it } from "vitest";
import type { DictionaryTerm } from "@weldspeak/protocol";
import { loadTerms } from "../src/routes/dictionary.js";
import { MEMBER_USER, ORG_ACME, acmeMember, call, resetDatabase, tokenFor } from "./helpers.js";

beforeEach(resetDatabase);

async function seedRow(term: string, options: { source?: string; createdAt?: string } = {}) {
  await env.DB.prepare(
    `INSERT INTO dictionary_terms (id, scope, clerk_user_id, clerk_org_id, term, source, created_at)
     VALUES (?, 'user', ?, NULL, ?, ?, ?)`,
  )
    .bind(
      crypto.randomUUID(),
      MEMBER_USER,
      term,
      options.source ?? "manual",
      options.createdAt ?? "2026-09-01 10:00:00",
    )
    .run();
}

async function listTerms(): Promise<string[]> {
  const response = await call("/api/dictionary", {
    token: await tokenFor(MEMBER_USER, acmeMember),
    org: ORG_ACME,
  });
  const { terms } = await response.json<{ terms: DictionaryTerm[] }>();
  return terms.map((term) => term.term);
}

async function learn(body: { heard?: string | null; meant: string }): Promise<Response> {
  return call("/api/dictionary/learn", {
    method: "POST",
    token: await tokenFor(MEMBER_USER, acmeMember),
    org: ORG_ACME,
    body,
  });
}

describe("learning", () => {
  it("ignores a word with no correction behind it", async () => {
    const response = await learn({ heard: null, meant: "Honestly" });

    // Acknowledged, so an older desktop build stops retrying it.
    expect(response.status).toBe(200);
    expect(await listTerms()).toEqual([]);
  });

  it("stores a correction with the misheard form as a hint", async () => {
    const response = await learn({ heard: "cloud code", meant: "Claude Code" });
    expect(response.status).toBe(201);

    const terms = await loadTerms(env.DB, MEMBER_USER, ORG_ACME);
    expect(terms).toMatchObject([{ term: "Claude Code", soundsLike: "cloud code" }]);
  });

  it("promotes a harvested word the person later corrects towards", async () => {
    await seedRow("Inconel", { source: "harvested" });
    await learn({ heard: "in colonel", meant: "Inconel" });

    expect(await listTerms()).toEqual(["Inconel"]);
  });
});

describe("harvested terms", () => {
  it("are left out of the dictionary and of dictation", async () => {
    await seedRow("Maybe", { source: "harvested" });
    await seedRow("Inconel 625");

    expect(await listTerms()).toEqual(["Inconel 625"]);
    const terms = await loadTerms(env.DB, MEMBER_USER, ORG_ACME);
    expect(terms.map((term) => term.term)).toEqual(["Inconel 625"]);
  });

  it("come back when the person adds the same term by hand", async () => {
    await seedRow("Sarah", { source: "harvested" });

    const response = await call("/api/dictionary", {
      method: "POST",
      token: await tokenFor(MEMBER_USER, acmeMember),
      org: ORG_ACME,
      body: { scope: "user", term: "Sarah" },
    });

    expect(response.status).toBe(201);
    expect(await listTerms()).toEqual(["Sarah"]);
  });
});

describe("migration 0003 backfill", () => {
  /** Re-run the migration's harvested backfill against seeded rows. */
  async function backfill(): Promise<void> {
    const migrations = (env as unknown as { TEST_MIGRATIONS: Array<{ name: string; queries: string[] }> })
      .TEST_MIGRATIONS;
    const migration = migrations.find((entry) => entry.name.startsWith("0003"));
    const update = migration?.queries.find((query) => query.includes("'harvested'") && query.startsWith("UPDATE"));
    expect(update).toBeDefined();
    await env.DB.prepare(update!).run();
  }

  async function seedTranscriptAt(text: string, createdAt: string) {
    await env.DB.prepare(
      `INSERT INTO transcripts (id, clerk_user_id, clerk_org_id, raw, formatted, duration_ms, created_at)
       VALUES (?, ?, NULL, ?, ?, 1000, ?)`,
    )
      .bind(crypto.randomUUID(), MEMBER_USER, text, text, createdAt)
      .run();
  }

  it("marks words uploaded right after the dictation they came from", async () => {
    await seedTranscriptAt("Maybe we ship on Monday. Honestly it is fine.", "2026-09-01 10:00:00");
    await seedRow("Maybe", { createdAt: "2026-09-01 10:00:03" });
    await seedRow("Honestly", { createdAt: "2026-09-01 10:00:03" });

    await backfill();

    expect(await listTerms()).toEqual([]);
  });

  it("leaves terms typed in by hand alone", async () => {
    await seedTranscriptAt("Send the Inconel 625 report.", "2026-09-01 10:00:00");
    // Added long after, and a term never dictated at all.
    await seedRow("Inconel 625", { createdAt: "2026-09-03 09:00:00" });
    await seedRow("WeldSuite", { createdAt: "2026-09-01 10:00:03" });

    await backfill();

    expect(await listTerms()).toEqual(["Inconel 625", "WeldSuite"]);
  });
});
