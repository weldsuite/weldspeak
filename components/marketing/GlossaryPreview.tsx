"use client";

const examples = [
  {
    heard: "in colonel six twenty five",
    typed: "Inconel 625",
    why: "Alloy designations from the shared glossary, boosted in the recognizer before cleanup even runs.",
    tone: "bg-[#F8E4D4] dark:bg-card",
  },
  {
    heard: "um so the uh traveler for delta three",
    typed: "the traveler for Delta 3",
    why: "Fillers and false starts come out. If cleanup misses its 2.5 s budget, the raw transcript still ships.",
    tone: "bg-[#E4F0E8] dark:bg-card",
  },
  {
    heard: "p number forty three to p number five a",
    typed: "P-No. 43 to P-No. 5A",
    why: "Part numbers, customer names, and procedure codes are team vocabulary — not generic speech-model guesses.",
    tone: "bg-[#F3E4EA] dark:bg-card",
  },
];

export function GlossaryPreview() {
  return (
    <section id="glossary" className="scroll-mt-24 py-20 md:py-28">
      <div className="container">
        <div className="grid gap-8 lg:grid-cols-[1.15fr_1fr] lg:items-end">
          <h2 className="font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] text-pretty lg:text-[3.75rem]">
            The glossary is the product.
          </h2>
          <p className="max-w-md text-[14px] leading-relaxed text-muted-foreground lg:justify-self-end">
            Generic dictation hears shop talk and invents English. WeldSpeak
            feeds your terms to the recognizer and the cleanup model so the
            words on the traveler are the words you said.
          </p>
        </div>
        <div className="mt-14 grid gap-4 md:grid-cols-3">
          {examples.map((example) => (
            <figure
              key={example.typed}
              className={`flex flex-col rounded-2xl p-6 ${example.tone}`}
            >
              <blockquote className="space-y-3 font-mono text-sm">
                <p className="text-muted-foreground line-through decoration-foreground/20">
                  {example.heard}
                </p>
                <p className="font-display text-lg font-semibold tracking-tight text-foreground">
                  {example.typed}
                </p>
              </blockquote>
              <figcaption className="mt-4 text-[13px] leading-relaxed text-muted-foreground">
                {example.why}
              </figcaption>
            </figure>
          ))}
        </div>
      </div>
    </section>
  );
}
