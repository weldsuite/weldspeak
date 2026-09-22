/**
 * Per-person WeldSpeak billing entitlements.
 *
 * Clerk Billing is the source of truth. Desktop JWTs carry a snapshot so the
 * dictation Durable Object can gate without another Clerk round-trip mid-
 * utterance. Refresh re-resolves so upgrades apply within one hour.
 */

import { createClerkClient } from "@clerk/backend";
import type { Env } from "../env.js";

/** Plan / feature slugs configured in the Clerk Dashboard. */
export const PLAN_WELDSPEAK = "weldspeak";
export const FEATURE_UNLIMITED_WORDS = "unlimited_words";
export const FEATURE_WELDSUITE = "weldsuite";

/** Free tier hard limit: words per calendar month (UTC). */
export const FREE_MONTHLY_WORD_CAP = 2000;

export type Entitlement = "free" | "paid" | "weldsuite";

export function isUnlimited(entitlement: Entitlement): boolean {
  return entitlement === "paid" || entitlement === "weldsuite";
}

export function monthlyWordCap(entitlement: Entitlement): number | null {
  return isUnlimited(entitlement) ? null : FREE_MONTHLY_WORD_CAP;
}

/** True when a free user has already used their monthly word allowance. */
export function isWordQuotaExceeded(entitlement: Entitlement, wordsUsed: number): boolean {
  if (isUnlimited(entitlement)) return false;
  return wordsUsed >= FREE_MONTHLY_WORD_CAP;
}

/** Whitespace-separated tokens on the final transcript. */
export function countWords(text: string): number {
  const trimmed = text.trim();
  if (!trimmed) return 0;
  return trimmed.split(/\s+/).filter(Boolean).length;
}

function hasFeatureSlug(
  features: Array<{ slug?: string | null }> | undefined,
  slug: string,
): boolean {
  return (features ?? []).some((feature) => feature.slug === slug);
}

/**
 * Resolve what a Clerk user is allowed to dictate this month.
 *
 * Order: WeldSuite grant (metadata or feature) → active paid plan → free.
 * Clerk Billing failures degrade to free rather than blocking dictation for
 * everyone during an outage of the billing API.
 */
export async function resolveEntitlement(env: Env, userId: string): Promise<Entitlement> {
  if (!env.CLERK_SECRET_KEY) return "free";

  const clerk = createClerkClient({ secretKey: env.CLERK_SECRET_KEY });

  try {
    const user = await clerk.users.getUser(userId);
    if (hasWeldsuiteMetadata(user)) {
      return "weldsuite";
    }
  } catch (error) {
    console.warn("entitlement: could not load user", userId, error);
  }

  try {
    const subscription = await clerk.billing.getUserBillingSubscription(userId);
    const items = subscription.subscriptionItems ?? [];

    for (const item of items) {
      const status = String(item.status);
      if (status !== "active" && status !== "trialing") continue;

      const plan = item.plan;
      if (!plan) continue;

      if (hasFeatureSlug(plan.features, FEATURE_WELDSUITE) || plan.slug === FEATURE_WELDSUITE) {
        return "weldsuite";
      }
      if (
        plan.slug === PLAN_WELDSPEAK ||
        hasFeatureSlug(plan.features, FEATURE_UNLIMITED_WORDS)
      ) {
        return "paid";
      }
    }
  } catch (error) {
    // No subscription / billing not enabled → free.
    console.warn("entitlement: billing lookup failed", userId, error);
  }

  return "free";
}

function hasWeldsuiteMetadata(user: {
  publicMetadata?: Record<string, unknown> | null;
  raw?: { public_metadata?: Record<string, unknown> | null } | null;
}): boolean {
  const meta = user.publicMetadata ?? user.raw?.public_metadata ?? null;
  return meta?.weldsuite === true || meta?.weldsuite === "true";
}

