import { Check } from "lucide-react";
import { OutlineCta, PrimaryCta } from "@/components/marketing/Cta";
import { DesktopDownloads } from "@/components/marketing/DesktopDownloads";
import { FinalCta } from "@/components/marketing/FinalCta";
import { PageIntro } from "@/components/marketing/PageIntro";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";
import { resolveDesktopInstallers } from "@/lib/desktop-downloads";

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

export async function DesktopPage() {
  const installers = await resolveDesktopInstallers();

  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />
      <PageIntro
        label="Windows and macOS"
        title="At home on your desktop."
        description="A tray app with a global hotkey. Hold a key, speak, let go. Cleaned-up text types into Slack, email, your IDE, or a browser form."
      />

      <DesktopDownloads
        version={installers.version}
        macUrl="/api/download/mac"
        windowsUrl="/api/download/windows"
      />

      <ol className="container mt-16 grid gap-10 border-t border-border pt-16 md:grid-cols-3">
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

      <div className="container mt-16 flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-card p-6">
        <Check className="size-4 shrink-0" strokeWidth={2} />
        <p className="mr-auto text-[13px] text-muted-foreground">
          After install, sign in and approve the device. Add the terms your shop
          actually says.
        </p>
        <PrimaryCta href="/sign-in">Sign in</PrimaryCta>
        <OutlineCta href="/pricing">See plans</OutlineCta>
      </div>

      <FinalCta
        title="Ready when you are."
        body="Pick your platform above. The download starts immediately — no releases page, no detour."
      />
      <SiteFooter />
    </div>
  );
}
