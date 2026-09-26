/**
 * Custom dictionary: the vocabulary boosts that keep "Inconel 625" from coming
 * back as "in colonel six twenty five".
 *
 * Terms live in two scopes. Personal terms belong to one user. Org terms are
 * the shared team glossary — every member reads them, only admins write them.
 * A dictation uses the union of both.
 */

import { Hono } from "hono";
import type { CreateTermRequest, DictionaryTerm, LearnCorrectionRequest } from "@weldspeak/protocol";
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
 * Harvested rows (see migrations/0003_term_source.sql) are left out: they are
 * words WeldSpeak picked out of its own output, not vocabulary anyone chose.
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
        WHERE ((scope = 'user' AND clerk_user_id = ?)
            OR (scope = 'org'  AND clerk_org_id = ?))
          AND source <> 'harvested'
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

  // A personal term an older desktop build harvested is hidden, not gone.
  // Typing it in by hand is a deliberate choice, so it comes back as manual
  // rather than failing as a duplicate of a row the person cannot see.
  if (scope === "user") {
    const revived = await c.env.DB.prepare(
      `UPDATE dictionary_terms
          SET source = 'manual', sounds_like = ?, created_at = datetime('now')
        WHERE scope = 'user' AND clerk_user_id = ? AND term = ? AND source = 'harvested'
        RETURNING id, created_at`,
    )
      .bind(soundsLike, userId, term)
      .first<{ id: string; created_at: string }>();
    if (revived) {
      return c.json(
        {
          id: revived.id,
          scope,
          term,
          soundsLike,
          createdAt: revived.created_at,
        } satisfies DictionaryTerm,
        201,
      );
    }
  }

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

dictionaryRoutes.post("/learn", async (c) => {
  const { userId } = c.get("auth");
  const body = await c.req.json<LearnCorrectionRequest>().catch(() => null);
  const meant = body?.meant?.trim();
  if (!meant) {
    return c.json({ error: "bad_request", message: "meant is required" }, 400);
  }
  if (meant.length > 128) {
    return c.json({ error: "bad_request", message: "term must be 128 characters or fewer" }, 400);
  }

  const heard = body?.heard?.trim() || null;
  if (heard && heard.length > 128) {
    return c.json({ error: "bad_request", message: "heard must be 128 characters or fewer" }, 400);
  }
  // Only a correction teaches anything: a word the person fixed after the
  // recognizer got it wrong. Older desktop builds also sent words picked out
  // of the dictation itself, with no `heard`; boosting those made recognition
  // worse with use. They are acknowledged so those builds stop retrying, and
  // otherwise ignored.
  if (!heard) {
    return c.json({ ok: true, ignored: true });
  }
  const soundsLike = heard.toLocaleLowerCase() !== meant.toLocaleLowerCase() ? heard : null;

  const existing = await c.env.DB.prepare(
    `SELECT id, sounds_like, created_at
       FROM dictionary_terms
      WHERE scope = 'user' AND clerk_user_id = ? AND term = ? COLLATE NOCASE`,
  )
    .bind(userId, meant)
    .first<{ id: string; sounds_like: string | null; created_at: string }>();

  if (existing) {
    const nextSounds = soundsLike ?? existing.sounds_like;
    // A harvested row the person has now corrected towards is real vocabulary.
    await c.env.DB.prepare(
      `UPDATE dictionary_terms
          SET sounds_like = ?,
              source = CASE source WHEN 'harvested' THEN 'correction' ELSE source END
        WHERE id = ?`,
    )
      .bind(nextSounds, existing.id)
      .run();
    return c.json({
      id: existing.id,
      scope: "user",
      term: meant,
      soundsLike: nextSounds,
      createdAt: existing.created_at,
    } satisfies DictionaryTerm);
  }

  const id = crypto.randomUUID();
  await c.env.DB.prepare(
    `INSERT INTO dictionary_terms (id, scope, clerk_user_id, clerk_org_id, term, sounds_like, source)
     VALUES (?, 'user', ?, NULL, ?, ?, 'correction')`,
  )
    .bind(id, userId, meant, soundsLike)
    .run();

  return c.json(
    {
      id,
      scope: "user",
      term: meant,
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
