"use client";

import { Keyboard, MessageSquareText, Mic } from "lucide-react";

const steps = [
  {
    icon: Keyboard,
    title: "Hold the key",
    body: "A global hotkey, from any app. The overlay appears so you know the mic is live — bars follow your voice, not a fake loop.",
  },
  {
    icon: Mic,
    title: "Speak",
    body: "Audio streams to the recognizer while you talk. The transcript is ready the moment you release, not after an upload.",
  },
  {
    icon: MessageSquareText,
    title: "It types",
    body: "Cleaned-up text is injected where the cursor already was. Slack, an email, an IDE, a browser form. No app switch.",
  },
];

export function HowItWorks() {
  return (
    <section id="how-it-works" className="scroll-mt-24 py-20 md:py-28">
      <div className="container">
        <div className="grid gap-8 lg:grid-cols-[1.15fr_1fr] lg:items-end">
          <div>
            <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
              Less typing. More welding.
            </p>
            <h2 className="mt-3 font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] text-pretty lg:text-[3.75rem]">
              Hold. Speak. Let go.
            </h2>
          </div>
          <p className="max-w-md text-[14px] leading-relaxed text-muted-foreground lg:justify-self-end">
            Dictation that keeps up with a shop floor, not a meeting transcript
            you clean up later.
          </p>
        </div>
        <ol className="mt-16 grid gap-10 md:grid-cols-3">
          {steps.map((step, index) => (
            <li key={step.title}>
              <step.icon className="size-6 text-foreground" strokeWidth={1.35} />
              <p className="mt-5 text-[11px] font-semibold tracking-[1.2px] text-muted-foreground">
                {String(index + 1).padStart(2, "0")}
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
      </div>
    </section>
  );
}
