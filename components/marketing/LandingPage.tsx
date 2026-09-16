"use client";

import { Check } from "lucide-react";
import { OutlineCta, PrimaryCta } from "@/components/marketing/Cta";
import { FinalCta } from "@/components/marketing/FinalCta";
import { HeroVisual } from "@/components/marketing/HeroVisual";
import {
  DESKTOP_PAGE,
  FEATURES_PAGE,
  HOW_IT_WORKS_PAGE,
  PRICING_PAGE,
  PRICE_PER_PERSON_MONTH,
  FREE_MONTHLY_WORD_CAP,
} from "@/components/marketing/links";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";
import { WhereItTypes } from "@/components/marketing/WhereItTypes";

function Asterisk({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 48 48"
      aria-hidden
      className={className}
      fill="currentColor"
    >
      <path d="M22.6 2.5h2.8l.4 16.2 13.7-8.7 1.4 2.4-13.4 9.4 13.4 9.4-1.4 2.4-13.7-8.7-.4 16.2h-2.8l-.4-16.2-13.7 8.7-1.4-2.4 13.4-9.4L7.1 12.4l1.4-2.4 13.7 8.7z" />
    </svg>
  );
}

/**
 * Home page — weldsuite.org-style: hero + short story + CTAs into real pages.
 * Full product detail lives on /features, /how-it-works, /pricing, /desktop.
 */
export function LandingPage() {
  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />

      <section className="container grid items-center gap-16 py-16 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)] lg:gap-10 lg:py-20">
        <div className="max-w-xl">
          <p className="inline-flex items-center gap-2 rounded-full border border-border bg-background px-3 py-1 text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
            <span className="size-1.5 rounded-full bg-emerald-500" />
            One shared glossary
          </p>
          <h1 className="mt-6 font-display text-[2.75rem] leading-[0.98] font-semibold tracking-[-0.06em] text-pretty sm:text-6xl lg:text-[5.4rem]">
            Hold a key.
            <span className="mt-1 flex items-end gap-3 text-brand sm:mt-2">
              Speak, let go.
              <Asterisk className="mb-1 hidden size-10 shrink-0 sm:block lg:mb-2 lg:size-14" />
            </span>
          </h1>
          <p className="mt-6 max-w-md text-[14px] leading-relaxed text-muted-foreground">
            Cleaned-up text appears wherever you were typing — Slack, an email,
            an IDE, a browser form. Built for shops that cannot afford “in
            colonel six twenty five”.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-2.5">
            <PrimaryCta href={DESKTOP_PAGE}>
              Download for Windows & macOS
            </PrimaryCta>
            <OutlineCta href="/sign-in">Sign in</OutlineCta>
          </div>
          <p className="mt-5 flex items-center gap-2 text-[12px] text-muted-foreground">
            <Check className="size-3.5" strokeWidth={2.25} />
            Free tier, a WeldSpeak subscription, or included with WeldSuite.
          </p>
        </div>
        <HeroVisual />
      </section>

      <p className="container pt-6 pb-2 text-[11px] tracking-[0.14em] text-muted-foreground uppercase">
        01 — Hold. Speak. Let go.
      </p>
      <WhereItTypes />

      <section className="py-20 md:py-28">
        <div className="container grid gap-10 lg:grid-cols-3">
          <a
            href={HOW_IT_WORKS_PAGE}
            className="group rounded-2xl border border-border bg-card p-8 transition-colors hover:border-foreground/30"
          >
            <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
              How it works
            </p>
            <h2 className="mt-3 font-display text-2xl font-semibold tracking-tight group-hover:text-brand">
              Hold. Speak. Let go.
            </h2>
            <p className="mt-3 text-[13px] leading-relaxed text-muted-foreground">
              A global hotkey, streaming recognition, and text injected where
              the cursor already was.
            </p>
          </a>
          <a
            href={FEATURES_PAGE}
            className="group rounded-2xl border border-border bg-card p-8 transition-colors hover:border-foreground/30"
          >
            <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
              Features
            </p>
            <h2 className="mt-3 font-display text-2xl font-semibold tracking-tight group-hover:text-brand">
              Built the way a shop talks.
            </h2>
            <p className="mt-3 text-[13px] leading-relaxed text-muted-foreground">
              Streaming speech-to-text, cleanup on a deadline, and a glossary
              that knows Inconel from English.
            </p>
          </a>
          <a
            href={PRICING_PAGE}
            className="group rounded-2xl border border-border bg-card p-8 transition-colors hover:border-foreground/30"
          >
            <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
              Pricing
            </p>
            <h2 className="mt-3 font-display text-2xl font-semibold tracking-tight group-hover:text-brand">
              ${PRICE_PER_PERSON_MONTH} / person, or WeldSuite.
            </h2>
            <p className="mt-3 text-[13px] leading-relaxed text-muted-foreground">
              Free is {FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words per
              month. Uncapped when you subscribe — or included with WeldSuite.
            </p>
          </a>
        </div>
      </section>

      <FinalCta />
      <SiteFooter />
    </div>
  );
}
