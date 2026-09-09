/**
 * Custom dictionary: the vocabulary boosts that keep "Inconel 625" from coming
 * back as "in colonel six twenty five".
 *
 * Terms live in two scopes. Personal terms belong to one user. Org terms are
 * the shared team glossary — every member reads them, only admins write them.
 * A dictation uses the union of both.
 */

import { Hono } from "hono";
import type { CreateTermRequest, DictionaryTerm } from "@weldspeak/protocol";
import type { AppBindings } from "../auth/middleware.js";
import { requireAuth } from "../auth/middleware.js";

interface TermRow {
  id: string;
  scope: "user" | "org";
  term: string;
  sounds_like: string | null;
  created_at: string;
}

const toTerm = (row: TermRow): DictionaryTerm => ({
  id: row.id,
  scope: row.scope,
  term: row.term,
  soundsLike: row.sounds_like,
  createdAt: row.created_at,
});

/**
 * Every term that applies to this user in this org, personal and shared.
 *
 * Shared by the HTTP route and the dictation session, so recognition boosts
 * and the cleanup prompt always draw on exactly the same vocabulary.
 */
export async function loadTerms(
  db: D1Database,
  userId: string,
  orgId: string | null,
): Promise<DictionaryTerm[]> {
  const { results } = await db
    .prepare(
      `SELECT id, scope, term, sounds_like, created_at
         FROM dictionary_terms
        WHERE (scope = 'user' AND clerk_user_id = ?)
           OR (scope = 'org'  AND clerk_org_id = ?)
        ORDER BY term COLLATE NOCASE`,
    )
    // An empty string never matches a real Clerk org ID, so personal-scope
    // callers simply get no org rows.
    .bind(userId, orgId ?? "")
    .all<TermRow>();

  return results.map(toTerm);
}

export const dictionaryRoutes = new Hono<AppBindings>();

dictionaryRoutes.use("*", requireAuth());

dictionaryRoutes.get("/", async (c) => {
  const { userId, activeOrg } = c.get("auth");
  return c.json({ terms: await loadTerms(c.env.DB, userId, activeOrg?.id ?? null) });
});

dictionaryRoutes.post("/", async (c) => {
  const { userId, activeOrg } = c.get("auth");
  const body = await c.req.json<CreateTermRequest>().catch(() => null);

  const term = body?.term?.trim();
  if (!term) {
    return c.json({ error: "bad_request", message: "term is required" }, 400);
  }
  if (term.length > 128) {
    return c.json({ error: "bad_request", message: "term must be 128 characters or fewer" }, 400);
  }

  const scope = body?.scope === "org" ? "org" : "user";

  if (scope === "org") {
    if (!activeOrg) {
      return c.json(
        { error: "bad_request", message: "Select an organization before adding a shared term" },
        400,
      );
    }
    // The dashboard hides this control from non-admins, but that only removes
    // markup — this is the check that enforces it.
    if (activeOrg.role !== "org:admin") {
      return c.json(
        { error: "org_forbidden", message: "Only organization admins can edit the shared glossary" },
        403,
      );
    }
  }

  const id = crypto.randomUUID();
  const soundsLike = body?.soundsLike?.trim() || null;

  try {
    await c.env.DB.prepare(
      `INSERT INTO dictionary_terms (id, scope, clerk_user_id, clerk_org_id, term, sounds_like)
       VALUES (?, ?, ?, ?, ?, ?)`,
    )
      .bind(
        id,
        scope,
        scope === "user" ? userId : null,
        scope === "org" ? activeOrg!.id : null,
        term,
        soundsLike,
      )
      .run();
  } catch (error) {
    // The unique indexes on (owner, term) make a duplicate a conflict rather
    // than a silent second row that would waste keyterm budget.
    if (String(error).includes("UNIQUE")) {
      return c.json({ error: "conflict", message: "That term already exists" }, 409);
    }
    throw error;
  }

  return c.json(
    {
      id,
      scope,
      term,
      soundsLike,
      createdAt: new Date().toISOString(),
    } satisfies DictionaryTerm,
    201,
  );
});

dictionaryRoutes.delete("/:id", async (c) => {
  const { userId, activeOrg } = c.get("auth");
  const id = c.req.param("id");

  // Scoped by owner in the WHERE clause, not checked after the fact: a delete
  // by ID alone would let any authenticated caller remove any other org's terms.
  const result = await c.env.DB.prepare(
    `DELETE FROM dictionary_terms
      WHERE id = ?
        AND ( (scope = 'user' AND clerk_user_id = ?)
           OR (scope = 'org'  AND clerk_org_id = ? AND ?) )`,
  )
    .bind(id, userId, activeOrg?.id ?? "", activeOrg?.role === "org:admin" ? 1 : 0)
    .run();

  if (result.meta.changes === 0) {
    return c.json({ error: "not_found", message: "No such term" }, 404);
  }
  return c.json({ ok: true });
});
