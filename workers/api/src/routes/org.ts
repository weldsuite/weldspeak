/**
 * Organization settings and usage.
 *
 * Both are admin-facing, but they differ in who may read them: any member can
 * read the settings that govern their own dictations (notably whether
 * transcripts are retained), while the per-member usage breakdown is
 * admin-only.
 */

import { Hono } from "hono";
import type { OrgSettings, UsageSummary } from "@weldspeak/protocol";
import type { AppBindings } from "../auth/middleware.js";
import { requireAuth, requireOrg, requireOrgAdmin } from "../auth/middleware.js";

interface SettingsRow {
  clerk_org_id: string;
  retain_transcripts: number;
  monthly_minute_cap: number | null;
}

/** Defaults for an org that has never had its settings touched. */
const DEFAULT_SETTINGS = (orgId: string): OrgSettings => ({
  orgId,
  retainTranscripts: true,
  monthlyMinuteCap: null,
});

/** Read an org's policy, falling back to defaults when no row exists yet. */
export async function loadOrgSettings(
  db: D1Database,
  orgId: string | null,
): Promise<OrgSettings | null> {
  if (!orgId) return null;

  const row = await db
    .prepare(
      `SELECT clerk_org_id, retain_transcripts, monthly_minute_cap
         FROM org_settings WHERE clerk_org_id = ?`,
    )
    .bind(orgId)
    .first<SettingsRow>();

  if (!row) return DEFAULT_SETTINGS(orgId);

  return {
    orgId: row.clerk_org_id,
    retainTranscripts: row.retain_transcripts === 1,
    monthlyMinuteCap: row.monthly_minute_cap,
  };
}

/** UTC month boundary, as `YYYY-MM-01`. Usage caps run on calendar months. */
export function currentPeriodStart(now = new Date()): string {
  return `${now.toISOString().slice(0, 7)}-01`;
}

/** Seconds of audio an org has used this calendar month. */
export async function orgUsageSeconds(db: D1Database, orgId: string | null): Promise<number> {
  const row = await db
    .prepare(
      `SELECT COALESCE(SUM(audio_seconds), 0) AS total
         FROM usage WHERE clerk_org_id = ? AND day >= ?`,
    )
    .bind(orgId ?? "", currentPeriodStart())
    .first<{ total: number }>();

  return row?.total ?? 0;
}

export const orgRoutes = new Hono<AppBindings>();

orgRoutes.use("*", requireAuth());

/** Readable by any member: clients must honour the retention policy locally. */
orgRoutes.get("/settings", requireOrg(), async (c) => {
  const { activeOrg } = c.get("auth");
  return c.json(await loadOrgSettings(c.env.DB, activeOrg!.id));
});

orgRoutes.patch("/settings", requireOrgAdmin(), async (c) => {
  const { activeOrg } = c.get("auth");
  const body = await c.req
    .json<Partial<Pick<OrgSettings, "retainTranscripts" | "monthlyMinuteCap">>>()
    .catch(() => null);

  if (!body) {
    return c.json({ error: "bad_request", message: "Invalid JSON body" }, 400);
  }

  const current = (await loadOrgSettings(c.env.DB, activeOrg!.id))!;
  const retainTranscripts = body.retainTranscripts ?? current.retainTranscripts;
  const monthlyMinuteCap =
    body.monthlyMinuteCap === undefined ? current.monthlyMinuteCap : body.monthlyMinuteCap;

  if (monthlyMinuteCap !== null && (!Number.isFinite(monthlyMinuteCap) || monthlyMinuteCap < 0)) {
    return c.json(
      { error: "bad_request", message: "monthlyMinuteCap must be a non-negative number or null" },
      400,
    );
  }

  await c.env.DB.prepare(
    `INSERT INTO org_settings (clerk_org_id, retain_transcripts, monthly_minute_cap, updated_at)
     VALUES (?, ?, ?, datetime('now'))
     ON CONFLICT (clerk_org_id) DO UPDATE SET
       retain_transcripts = excluded.retain_transcripts,
       monthly_minute_cap = excluded.monthly_minute_cap,
       updated_at = excluded.updated_at`,
  )
    .bind(activeOrg!.id, retainTranscripts ? 1 : 0, monthlyMinuteCap)
    .run();

  return c.json({
    orgId: activeOrg!.id,
    retainTranscripts,
    monthlyMinuteCap,
  } satisfies OrgSettings);
});

orgRoutes.get("/usage", async (c) => {
  const { userId, activeOrg } = c.get("auth");
  const orgId = activeOrg?.id ?? null;
  const periodStart = currentPeriodStart();

  const settings = await loadOrgSettings(c.env.DB, orgId);
  const audioSeconds = await orgUsageSeconds(c.env.DB, orgId);

  // Non-admins see only their own consumption; the per-member breakdown is an
  // admin view.
  const isAdmin = activeOrg?.role === "org:admin";
  const { results } = isAdmin
    ? await c.env.DB.prepare(
        `SELECT clerk_user_id AS userId, SUM(audio_seconds) AS audioSeconds
           FROM usage WHERE clerk_org_id = ? AND day >= ?
          GROUP BY clerk_user_id ORDER BY audioSeconds DESC`,
      )
        .bind(orgId ?? "", periodStart)
        .all<{ userId: string; audioSeconds: number }>()
    : await c.env.DB.prepare(
        `SELECT clerk_user_id AS userId, SUM(audio_seconds) AS audioSeconds
           FROM usage WHERE clerk_org_id = ? AND day >= ? AND clerk_user_id = ?
          GROUP BY clerk_user_id`,
      )
        .bind(orgId ?? "", periodStart, userId)
        .all<{ userId: string; audioSeconds: number }>();

  return c.json({
    orgId,
    periodStart,
    audioSeconds,
    monthlyMinuteCap: settings?.monthlyMinuteCap ?? null,
    byUser: results,
  } satisfies UsageSummary);
});
