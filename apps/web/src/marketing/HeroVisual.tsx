import { useState } from "react";
import {
  BookOpen,
  Globe,
  Mail,
  MessageSquare,
  Sparkles,
  Terminal,
} from "lucide-react";
import { cn } from "cn";

const views = [
  {
    id: "traveler",
    label: "Traveler",
    eyebrow: "WPS-441 · nozzle to shell",
    body: "Weld Inconel 625 overlay on the 2.25Cr-1Mo shell, P-No. 43 to P-No. 5A. Preheat 200 °F. Customer: Delta Forge.",
    aside: "Filler metal ERNiCrMo-3.",
  },
  {
    id: "slack",
    label: "Slack",
    eyebrow: "#shop-floor · traveler 1847",
    body: "Need ERNiCrMo-3 on the Inconel 625 overlay before second shift. Delta Forge is waiting on the traveler.",
    aside: "Typed into the thread that already had focus.",
  },
  {
    id: "ide",
    label: "IDE",
    eyebrow: "notes.md · job 1847",
    body: "Preheat 200 °F. P-No. 43 to P-No. 5A. Hold the key, speak the procedure, keep writing in the file.",
    aside: "No copy-paste. No app switch.",
  },
] as const;

const floatIcons = [
  { icon: MessageSquare, className: "bg-[#F8C4B4] text-[#9A3B28]" },
  { icon: Mail, className: "bg-[#F5D0A8] text-[#B45A1A]" },
  { icon: Terminal, className: "bg-[#D7C8F0] text-[#5B3F8A]" },
  { icon: Globe, className: "bg-[#C8E8D4] text-[#2F6B48]" },
];

export function HeroVisual() {
  const [active, setActive] = useState<(typeof views)[number]["id"]>("traveler");
  const view = views.find((item) => item.id === active) ?? views[0];

  return (
    <div className="relative mx-auto w-full max-w-[640px] lg:mx-0 lg:max-w-none">
      <ul
        className="pointer-events-none absolute -top-7 right-6 z-10 hidden gap-2.5 sm:flex"
        aria-hidden
      >
        {floatIcons.map((item, index) => (
          <li
            key={item.className}
            className={cn(
              "flex size-11 items-center justify-center rounded-2xl shadow-md",
              item.className,
            )}
            style={{ transform: `translateY(${index % 2 === 0 ? 0 : 10}px)` }}
          >
            <item.icon className="size-4" strokeWidth={1.75} />
          </li>
        ))}
      </ul>

      <div className="overflow-hidden rounded-[22px] border border-border bg-card shadow-xl">
        <div className="flex items-center gap-2 border-b border-border px-4 py-2.5">
          <span className="size-2.5 rounded-full bg-[#E8B4A8]" />
          <span className="size-2.5 rounded-full bg-[#E8D4A0]" />
          <span className="size-2.5 rounded-full bg-[#C8E0C0]" />
          <span className="ml-3 flex-1 truncate rounded-full bg-muted px-3 py-1 text-center text-[11px] text-muted-foreground">
            weldspeak.com
          </span>
          <span className="hidden text-[10px] font-medium tracking-[0.12em] text-muted-foreground uppercase sm:inline">
            Interactive preview
          </span>
        </div>

        <div className="grid min-h-[340px] md:grid-cols-[11.5rem_1fr]">
          <aside className="hidden border-r border-border p-4 md:block">
            <p className="mb-3 text-[10px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              Workspace
            </p>
            <ul className="space-y-1 text-[13px]">
              {views.map((item) => (
                <li key={item.id}>
                  <button
                    type="button"
                    onClick={() => setActive(item.id)}
                    className={cn(
                      "w-full rounded-md px-2.5 py-1.5 text-left transition-colors",
                      item.id === active
                        ? "bg-muted font-medium text-foreground"
                        : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
                    )}
                  >
                    {item.label}
                  </button>
                </li>
              ))}
              <li className="px-2.5 py-1.5 text-muted-foreground">Dictionary</li>
              <li className="px-2.5 py-1.5 text-muted-foreground">History</li>
            </ul>
          </aside>

          <div className="relative flex flex-col p-5 md:p-6">
            <div className="mb-4 flex gap-1 md:hidden">
              {views.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setActive(item.id)}
                  className={cn(
                    "rounded-full px-3 py-1 text-[11px] font-medium",
                    item.id === active
                      ? "bg-foreground text-background"
                      : "bg-muted text-muted-foreground",
                  )}
                >
                  {item.label}
                </button>
              ))}
            </div>

            <p className="text-[11px] text-muted-foreground">{view.eyebrow}</p>
            <h3 className="mt-3 font-display text-xl font-semibold tracking-tight">
              {view.label === "Traveler" ? "Job 1847" : view.label}
            </h3>
            <p className="mt-4 max-w-md text-[14px] leading-relaxed text-foreground/90">
              {view.body}
            </p>
            <p className="mt-3 text-[13px] text-muted-foreground">{view.aside}</p>

            <div className="mt-auto flex items-end justify-between pt-8">
              <span className="inline-flex items-center gap-2 rounded-full bg-foreground px-3 py-1.5 text-[11px] font-medium text-background">
                <span className="flex items-end gap-0.5" aria-hidden>
                  <span className="h-2 w-0.5 animate-pulse bg-brand" />
                  <span className="h-3 w-0.5 animate-pulse bg-brand delay-75" />
                  <span className="h-4 w-0.5 animate-pulse bg-brand" />
                  <span className="h-2.5 w-0.5 animate-pulse bg-brand delay-150" />
                </span>
                Listening
              </span>
              <span className="flex items-center gap-1 text-[11px] text-muted-foreground">
                <BookOpen className="size-3" />
                Inconel 625
              </span>
            </div>
          </div>
        </div>
      </div>

      <div className="absolute -right-2 -bottom-5 z-10 hidden max-w-[220px] rounded-xl border border-border bg-card p-3 shadow-lg sm:block">
        <p className="flex items-center gap-1.5 text-[12px] font-semibold">
          Same cursor. Shared glossary.
          <Sparkles className="size-3.5 text-brand" />
        </p>
        <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
          Types into whichever app already has focus.
        </p>
      </div>
    </div>
  );
}
