/**
 * Dictation history.
 *
 * Scoped to the caller: people see their own dictations and nobody else's,
 * admins included. An org admin can set retention policy and see aggregate
 * usage, but reading colleagues' dictated text is surveillance, not
 * administration, so there is deliberately no endpoint for it.
 */

import { Hono } from "hono";
import type { TranscriptRecord } from "@weldspeak/protocol";
import type { AppBindings } from "../auth/middleware.js";
import { requireAuth } from "../auth/middleware.js";

interface TranscriptRow {
  id: string;
  raw: string;
  formatted: string;
  duration_ms: number;
  app_name: string | null;
  created_at: string;
}

export const transcriptRoutes = new Hono<AppBindings>();

transcriptRoutes.use("*", requireAuth());

transcriptRoutes.get("/", async (c) => {
  const { userId } = c.get("auth");

  const limit = Math.min(Number(c.req.query("limit") ?? 50) || 50, 200);
  const search = c.req.query("q")?.trim();

  const { results } = search
    ? await c.env.DB.prepare(
        `SELECT id, raw, formatted, duration_ms, app_name, created_at
           FROM transcripts
          WHERE clerk_user_id = ? AND formatted LIKE ?
          ORDER BY created_at DESC LIMIT ?`,
      )
        .bind(userId, `%${search}%`, limit)
        .all<TranscriptRow>()
    : await c.env.DB.prepare(
        `SELECT id, raw, formatted, duration_ms, app_name, created_at
           FROM transcripts
          WHERE clerk_user_id = ?
          ORDER BY created_at DESC LIMIT ?`,
      )
        .bind(userId, limit)
        .all<TranscriptRow>();

  const transcripts: TranscriptRecord[] = results.map((row) => ({
    id: row.id,
    raw: row.raw,
    formatted: row.formatted,
    durationMs: row.duration_ms,
    appName: row.app_name,
    createdAt: row.created_at,
  }));

  return c.json({ transcripts });
});

transcriptRoutes.delete("/:id", async (c) => {
  const { userId } = c.get("auth");

  const result = await c.env.DB.prepare(
    `DELETE FROM transcripts WHERE id = ? AND clerk_user_id = ?`,
  )
    .bind(c.req.param("id"), userId)
    .run();

  if (result.meta.changes === 0) {
    return c.json({ error: "not_found", message: "No such transcript" }, 404);
  }
  return c.json({ ok: true });
});

/** Clear the caller's entire history. */
transcriptRoutes.delete("/", async (c) => {
  const { userId } = c.get("auth");
  const result = await c.env.DB.prepare(`DELETE FROM transcripts WHERE clerk_user_id = ?`)
    .bind(userId)
    .run();
  return c.json({ ok: true, deleted: result.meta.changes });
});
