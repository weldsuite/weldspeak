"use client";

import { useEffect, useState } from "react";
import { Apple, Check, Download, Monitor } from "lucide-react";
import { OutlineCta, PrimaryCta } from "@/components/marketing/Cta";
import { RELEASES_URL } from "@/components/marketing/links";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";

type Platform = "win" | "mac" | "other";

function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "other";
  const ua = navigator.userAgent;
  if (/Mac|iPhone|iPad/.test(ua)) return "mac";
  if (/Win/.test(ua)) return "win";
  return "other";
}

const steps = [
  {
    n: "01",
    title: "Install the tray app",
    body: "Run the Windows installer or open the macOS disk image. WeldSpeak lives in the menu bar / system tray — not as another window you have to live in.",
  },
  {
    n: "02",
    title: "Sign in on the web",
    body: "Same Clerk account as the rest of WeldSuite. The desktop app cannot hold a browser cookie, so you approve the device with a short code.",
  },
  {
    n: "03",
    title: "Hold a key and talk",
    body: "Put the terms your shop actually says in the glossary. Text appears wherever the cursor already was.",
  },
];

export function DownloadPage() {
  const [platform, setPlatform] = useState<Platform>("other");

  useEffect(() => {
    setPlatform(detectPlatform());
    document.title = "Download WeldSpeak — Windows and macOS";
    return () => {
      document.title = "WeldSpeak — Hold a key, speak, let go";
    };
  }, []);

  const recommended =
    platform === "mac" ? "macOS" : platform === "win" ? "Windows" : null;

  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />
      <article className="container py-16 md:py-24">
        <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
          Windows and macOS
        </p>
        <h1 className="mt-3 max-w-3xl font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] text-pretty lg:text-[4.5rem]">
          Download WeldSpeak.
        </h1>
        <p className="mt-5 max-w-lg text-[14px] leading-relaxed text-muted-foreground">
          A tray app with a global hotkey. Hold a key, speak, let go. Cleaned-up
          text types into Slack, email, your IDE, or a browser form.
          {recommended ? ` Looks like you are on ${recommended}.` : null}
        </p>

        <div className="mt-12 grid gap-4 md:grid-cols-2">
          <a
            href={RELEASES_URL}
            className={`flex flex-col rounded-2xl border bg-card p-8 transition-colors hover:border-foreground/30 ${
              platform === "win" ? "border-foreground" : "border-border"
            }`}
          >
            <Monitor className="size-6" strokeWidth={1.5} />
            <h2 className="mt-5 font-display text-2xl font-semibold tracking-tight">
              Windows
            </h2>
            <p className="mt-2 text-[13px] text-muted-foreground">
              NSIS installer. Current-user install, no admin required.
            </p>
            <span className="mt-8 inline-flex items-center gap-1.5 text-[11px] font-semibold">
              Download .exe
              <Download className="size-3.5" />
            </span>
          </a>
          <a
            href={RELEASES_URL}
            className={`flex flex-col rounded-2xl border bg-card p-8 transition-colors hover:border-foreground/30 ${
              platform === "mac" ? "border-foreground" : "border-border"
            }`}
          >
            <Apple className="size-6" strokeWidth={1.5} />
            <h2 className="mt-5 font-display text-2xl font-semibold tracking-tight">
              macOS
            </h2>
            <p className="mt-2 text-[13px] text-muted-foreground">
              Disk image for Apple silicon and Intel. Menu bar app, global
              hotkey.
            </p>
            <span className="mt-8 inline-flex items-center gap-1.5 text-[11px] font-semibold">
              Download .dmg
              <Download className="size-3.5" />
            </span>
          </a>
        </div>

        <p className="mt-6 text-[13px] text-muted-foreground">
          Installers are published with each GitHub release. Already on
          WeldSuite? Sign in with the same account — WeldSpeak is included in
          the package.
        </p>

        <ol className="mt-16 grid gap-10 border-t border-border pt-16 md:grid-cols-3">
          {steps.map((step) => (
            <li key={step.n}>
              <p className="text-[11px] font-semibold tracking-[1.2px] text-muted-foreground">
                {step.n}
              </p>
              <h3 className="mt-2 font-display text-xl font-semibold tracking-tight">
                {step.title}
              </h3>
              <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
                {step.body}
              </p>
            </li>
          ))}
        </ol>

        <div className="mt-16 flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-card p-6">
          <Check className="size-4 shrink-0" strokeWidth={2} />
          <p className="mr-auto text-[13px] text-muted-foreground">
            After install, sign in and approve the device. Add the terms your
            shop actually says.
          </p>
          <PrimaryCta href="/sign-in">Sign in</PrimaryCta>
          <OutlineCta href="/pricing">See plans</OutlineCta>
        </div>
      </article>
      <SiteFooter />
    </div>
  );
}
