import { describe, expect, it } from "vitest";
import {
  countWords,
  FREE_MONTHLY_WORD_CAP,
  isUnlimited,
  isWordQuotaExceeded,
  monthlyWordCap,
  resolveEntitlement,
} from "../src/billing/entitlements.js";
import { env } from "cloudflare:test";
import { resetClerkStub, stubClerk } from "./clerk-stub.js";

describe("countWords", () => {
  it("counts whitespace-separated tokens", () => {
    expect(countWords("hello world")).toBe(2);
    expect(countWords("  Inconel  625  ")).toBe(2);
    expect(countWords("")).toBe(0);
    expect(countWords("   ")).toBe(0);
  });
});

describe("word quota helpers", () => {
  it("caps free users at 2000 words", () => {
    expect(monthlyWordCap("free")).toBe(FREE_MONTHLY_WORD_CAP);
    expect(monthlyWordCap("paid")).toBeNull();
    expect(monthlyWordCap("weldsuite")).toBeNull();
    expect(isUnlimited("free")).toBe(false);
    expect(isUnlimited("paid")).toBe(true);
  });

  it("blocks free users at or over the cap", () => {
    expect(isWordQuotaExceeded("free", FREE_MONTHLY_WORD_CAP - 1)).toBe(false);
    expect(isWordQuotaExceeded("free", FREE_MONTHLY_WORD_CAP)).toBe(true);
    expect(isWordQuotaExceeded("free", FREE_MONTHLY_WORD_CAP + 50)).toBe(true);
  });

  it("never blocks paid or weldsuite by word cap", () => {
    expect(isWordQuotaExceeded("paid", 50_000)).toBe(false);
    expect(isWordQuotaExceeded("weldsuite", 50_000)).toBe(false);
  });
});

describe("resolveEntitlement", () => {
  it("returns free when there is no subscription", async () => {
    stubClerk({});
    await expect(resolveEntitlement(env as never, "user_free")).resolves.toBe("free");
    resetClerkStub();
  });

  it("returns paid for an active weldspeak plan", async () => {
    const clerk = stubClerk({});
    clerk.setBilling("user_paid", {
      kind: "plan",
      slug: "weldspeak",
      features: ["unlimited_words"],
    });
    await expect(resolveEntitlement(env as never, "user_paid")).resolves.toBe("paid");
    resetClerkStub();
  });

  it("returns weldsuite from public metadata", async () => {
    const clerk = stubClerk({});
    clerk.setWeldsuite("user_suite", true);
    await expect(resolveEntitlement(env as never, "user_suite")).resolves.toBe("weldsuite");
    resetClerkStub();
  });

  it("returns weldsuite from a granted feature on a subscription item", async () => {
    const clerk = stubClerk({});
    clerk.setBilling("user_suite_feat", {
      kind: "plan",
      slug: "custom",
      features: ["weldsuite"],
    });
    await expect(resolveEntitlement(env as never, "user_suite_feat")).resolves.toBe("weldsuite");
    resetClerkStub();
  });
});
