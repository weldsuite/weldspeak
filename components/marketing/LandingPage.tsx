"use client";

import { useEffect } from "react";
import {
  BookOpen,
  Check,
  Keyboard,
  Mic,
  Minus,
  Plus,
  Shield,
  Sparkles,
  Users,
} from "lucide-react";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { OutlineCta, PrimaryCta } from "@/components/marketing/Cta";
import { GlossaryPreview } from "@/components/marketing/GlossaryPreview";
import { HeroVisual } from "@/components/marketing/HeroVisual";
import { HowItWorks } from "@/components/marketing/HowItWorks";
import {
  DOWNLOAD_PAGE,
  FREE_MONTHLY_WORD_CAP,
  PRICE_PER_PERSON_MONTH,
  PRICING_PAGE,
  SIGN_IN_FOR_PRICING,
  WELDSUITE_PRICING_URL,
} from "@/components/marketing/links";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";
import { WhereItTypes } from "@/components/marketing/WhereItTypes";

const features = [
  {
    icon: Mic,
    title: "Streaming, not record-then-upload",
    description:
      "Audio goes to the recognizer while you speak, so the text is ready the moment you release the key.",
    tone: "bg-[#F8E4D4] dark:bg-card",
  },
  {
    icon: Sparkles,
    title: "Cleanup with a deadline",
    description:
      "Fillers and false starts come out. If the polish misses 2.5 seconds, the raw transcript still ships. Late is worse than scruffy.",
    tone: "bg-[#E4F0E8] dark:bg-card",
  },
  {
    icon: BookOpen,
    title: "A glossary that does work",
    description:
      "Alloy designations, customer names, part numbers. Fed to the recognizer as keyterm boosts and to cleanup as spelling context.",
    tone: "bg-[#F3E4EA] dark:bg-card",
  },
  {
    icon: Users,
    title: "Organizations, not personal accounts",
    description:
      "Shared vocabulary lives on the crew. Admins edit the org list; members get it the next time they dictate.",
    tone: "bg-[#E8E6F4] dark:bg-card",
  },
  {
    icon: Keyboard,
    title: "Windows and macOS",
    description:
      "A tray app with a global hotkey. Sign in once in the browser, approve the device, keep working in the tools you already have.",
    tone: "bg-[#F6EFE0] dark:bg-card",
  },
  {
    icon: Shield,
    title: "Your team, isolated",
    description:
      "Transcripts and glossaries are scoped to the organization. One shop cannot read another shop's words.",
    tone: "bg-[#E4EEF4] dark:bg-card",
  },
];

const faqs = [
  {
    id: "faq-what",
    question: "What is WeldSpeak?",
    answer:
      "A push-to-talk dictation app. Hold a key, speak, let go. Cleaned-up text is typed into whichever app had focus — Slack, email, an IDE, a browser form. No copy-paste, no app switch.",
  },
  {
    id: "faq-why",
    question: "Why not use the dictation built into the OS?",
    answer:
      "Generic recognizers hear shop vocabulary and invent English. WeldSpeak streams to a speech model, then a cleanup pass, and both of those see your glossary: Inconel 625, customer names, P-numbers. That is the difference between usable traveler text and a mess you have to edit.",
  },
  {
    id: "faq-offline",
    question: "Does it work offline?",
    answer:
      "No. Recognition and cleanup run on Cloudflare Workers AI. The desktop app captures the microphone and injects text; the Worker does the listening.",
  },
  {
    id: "faq-platforms",
    question: "Which platforms?",
    answer:
      "Windows and macOS. Installers are built on GitHub Actions. You do not need Rust on the machine that will run WeldSpeak.",
  },
  {
    id: "faq-sign-in",
    question: "Why do I sign in on the website?",
    answer:
      "Accounts are Clerk organizations. Session cookies live in the browser, so the desktop app cannot hold one. You sign in here, the app shows a short code, you approve the device, and the Worker mints its own tokens.",
  },
  {
    id: "faq-glossary",
    question: "Who can edit the shared glossary?",
    answer:
      "Organization admins. Personal terms stay private. Shared terms are the high-value list — the words a whole crew needs spelled the same way.",
  },
  {
    id: "faq-price",
    question: "What does it cost?",
    answer: `Free is ${FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words per calendar month. WeldSpeak is $${PRICE_PER_PERSON_MONTH} per person per month for uncapped words — each teammate who dictates pays their own subscription. If you already have a WeldSuite package, WeldSpeak is included.`,
  },
  {
    id: "faq-suite",
    question: "I already have WeldSuite. Do I need another subscription?",
    answer:
      "No. WeldSpeak is included with the WeldSuite package. Sign in with your WeldSuite account, download the app, and approve the device.",
  },
];

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

export function LandingPage() {
  useEffect(() => {
    const id = window.location.hash.replace(/^#/, "");
    if (!id) return;
    document.getElementById(id)?.scrollIntoView();
  }, []);

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
            <PrimaryCta href={DOWNLOAD_PAGE}>
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
      <HowItWorks />

      <section id="features" className="scroll-mt-24 py-20 md:py-28">
        <div className="container">
          <div className="grid gap-8 lg:grid-cols-[1.15fr_1fr] lg:items-end">
            <div>
              <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
                Built for the floor
              </p>
              <h2 className="mt-3 font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] text-pretty lg:text-[3.75rem]">
                Built the way a shop actually talks.
              </h2>
            </div>
            <p className="max-w-md text-[14px] leading-relaxed text-muted-foreground lg:justify-self-end">
              Streaming speech-to-text, cleanup on a deadline, and a glossary
              that knows Inconel from English. The rest is a tray app and a
              hotkey.
            </p>
          </div>
          <ul className="mt-14 grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {features.map((feature) => (
              <li
                key={feature.title}
                className={`rounded-2xl p-6 ${feature.tone}`}
              >
                <feature.icon className="size-5" strokeWidth={1.5} />
                <h3 className="mt-5 font-display text-lg font-semibold tracking-tight">
                  {feature.title}
                </h3>
                <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
                  {feature.description}
                </p>
              </li>
            ))}
          </ul>
        </div>
      </section>

      <GlossaryPreview />

      <section id="pricing" className="scroll-mt-24 py-20 md:py-28">
        <div className="container">
          <div className="grid gap-8 lg:grid-cols-[1.15fr_1fr] lg:items-end">
            <h2 className="font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] text-pretty lg:text-[3.75rem]">
              ${PRICE_PER_PERSON_MONTH} per person. Or bring WeldSuite.
            </h2>
            <p className="max-w-md text-[14px] leading-relaxed text-muted-foreground lg:justify-self-end">
              Free is {FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words per
              month. Subscribe when you need uncapped dictation — each person
              who uses WeldSpeak pays their own ${PRICE_PER_PERSON_MONTH}. Already
              on WeldSuite? It is included.
            </p>
          </div>
          <div className="mt-12 grid gap-4 lg:grid-cols-3">
            <article className="rounded-2xl border border-border bg-card p-8">
              <p className="text-[11px] font-semibold tracking-[1.2px] text-muted-foreground uppercase">
                Free
              </p>
              <p className="mt-4 font-display text-3xl font-semibold tracking-tight">
                {FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words / mo
              </p>
              <p className="mt-2 text-[13px] text-muted-foreground">
                For one person trying dictation in the tools they already use.
              </p>
              <ul className="mt-6 space-y-2.5 text-[13px]">
                <li>Windows and macOS desktop app</li>
                <li>Push-to-talk streaming dictation</li>
                <li>Cleanup on a 2.5 s deadline</li>
                <li>Personal glossary</li>
              </ul>
              <div className="mt-8">
                <OutlineCta href={DOWNLOAD_PAGE}>Download</OutlineCta>
              </div>
            </article>
            <article className="rounded-2xl border border-foreground bg-card p-8">
              <p className="text-[11px] font-semibold tracking-[1.2px] text-muted-foreground uppercase">
                WeldSpeak
              </p>
              <p className="mt-4 font-display text-3xl font-semibold tracking-tight">
                ${PRICE_PER_PERSON_MONTH} / person / mo
              </p>
              <p className="mt-2 text-[13px] text-muted-foreground">
                Uncapped words for each person who dictates. Shared glossary,
                usage, and team controls for shops that only need dictation.
              </p>
              <ul className="mt-6 space-y-2.5 text-[13px]">
                <li>Everything in Free, uncapped words</li>
                <li>Shared organization glossary</li>
                <li>Usage visibility</li>
                <li>Transcript retention you control</li>
              </ul>
              <div className="mt-8">
                <PrimaryCta href={SIGN_IN_FOR_PRICING}>Subscribe</PrimaryCta>
              </div>
            </article>
            <article className="rounded-2xl border border-border bg-card p-8">
              <p className="text-[11px] font-semibold tracking-[1.2px] text-muted-foreground uppercase">
                WeldSuite
              </p>
              <p className="mt-4 font-display text-3xl font-semibold tracking-tight">
                Included
              </p>
              <p className="mt-2 text-[13px] text-muted-foreground">
                Already on a WeldSuite package? WeldSpeak comes with it. Sign
                in with the same account — no second subscription.
              </p>
              <ul className="mt-6 space-y-2.5 text-[13px]">
                <li>Full WeldSpeak for the workspace</li>
                <li>Same Clerk account as the suite</li>
                <li>Shared org glossary and usage</li>
                <li>The rest of the WeldSuite apps</li>
              </ul>
              <div className="mt-8">
                <OutlineCta href={WELDSUITE_PRICING_URL} external>
                  See WeldSuite plans
                </OutlineCta>
              </div>
            </article>
          </div>
          <p className="mt-8 text-center text-[13px] text-muted-foreground">
            Ready to pay?{" "}
            <a href={PRICING_PAGE} className="underline underline-offset-2">
              Open checkout
            </a>
            .
          </p>
        </div>
      </section>

      <section id="faq" className="scroll-mt-24 py-20 md:py-28">
        <div className="container grid gap-12 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]">
          <h2 className="font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] lg:text-[3.75rem]">
            Good to know.
          </h2>
          <Accordion type="single" collapsible className="border-t border-border">
            {faqs.map((item) => (
              <AccordionItem key={item.id} value={item.id} className="border-border">
                <AccordionTrigger className="py-5 text-[15px] font-medium hover:no-underline [&_[data-slot=accordion-trigger-icon]]:hidden">
                  {item.question}
                  <Plus className="ml-auto size-4 shrink-0 group-aria-expanded/accordion-trigger:hidden" />
                  <Minus className="ml-auto hidden size-4 shrink-0 group-aria-expanded/accordion-trigger:inline" />
                </AccordionTrigger>
                <AccordionContent className="pb-5 text-[13px] leading-relaxed text-muted-foreground">
                  {item.answer}
                </AccordionContent>
              </AccordionItem>
            ))}
          </Accordion>
        </div>
      </section>

      <section className="py-20 md:py-28">
        <div className="container text-center">
          <h2 className="font-display text-5xl leading-[0.98] font-semibold tracking-[-0.06em] text-pretty lg:text-[5.5rem]">
            Talk. It types.
          </h2>
          <p className="mx-auto mt-6 max-w-md text-[14px] leading-relaxed text-muted-foreground">
            Download the app, sign in, approve the device, add the terms your
            shop actually says. That is the whole setup.
          </p>
          <div className="mt-8 flex flex-wrap items-center justify-center gap-2.5">
            <PrimaryCta href={DOWNLOAD_PAGE}>Download</PrimaryCta>
            <OutlineCta href="/sign-in">Sign in</OutlineCta>
          </div>
        </div>
      </section>

      <SiteFooter />
    </div>
  );
}
