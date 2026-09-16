"use client";

import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";

export function PrivacyPage() {
  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />
      <article className="container max-w-2xl py-16">
        <h1 className="font-display text-4xl font-semibold tracking-[-0.04em] lg:text-5xl">
          Privacy
        </h1>
        <p className="mt-4 text-[14px] text-muted-foreground">
          Last updated 14 September 2026. This is the honest version, not a
          lawyer&apos;s.
        </p>
        <div className="mt-10 space-y-8 text-[14px] leading-relaxed">
          <section className="space-y-3">
            <h2 className="font-display text-xl font-semibold tracking-tight">
              What we hear
            </h2>
            <p>
              The desktop app streams microphone audio to a Cloudflare Worker
              while you hold the dictation key. That audio is sent to Workers AI
              for speech-to-text, then a cleanup model. Audio is not kept as a
              recording after the session.
            </p>
          </section>
          <section className="space-y-3">
            <h2 className="font-display text-xl font-semibold tracking-tight">
              Transcripts
            </h2>
            <p>
              Whether a transcript is stored is an organization setting. Admins
              can turn retention off. When it is on, members of that
              organization can read the history in the dashboard. Other
              organizations cannot.
            </p>
          </section>
          <section className="space-y-3">
            <h2 className="font-display text-xl font-semibold tracking-tight">
              Accounts
            </h2>
            <p>
              Sign-in is Clerk, the same application as the rest of WeldSuite.
              We see the user and organization Clerk already has — not a
              separate WeldSpeak identity pile.
            </p>
          </section>
          <section className="space-y-3">
            <h2 className="font-display text-xl font-semibold tracking-tight">
              Glossary
            </h2>
            <p>
              Personal terms stay on your user. Shared terms belong to the
              organization. They exist so the recognizer spells the words you
              actually say.
            </p>
          </section>
        </div>
      </article>
      <SiteFooter />
    </div>
  );
}
