import { FinalCta } from "@/components/marketing/FinalCta";
import { GlossaryPreview } from "@/components/marketing/GlossaryPreview";
import { PageIntro } from "@/components/marketing/PageIntro";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";
import { features } from "@/lib/marketing-content";

export function FeaturesPage() {
  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />
      <PageIntro
        label="Built for the floor"
        title="Built the way a shop actually talks."
        description="Streaming speech-to-text, cleanup on a deadline, and a glossary that knows Inconel from English. The rest is a tray app and a hotkey."
      />

      <section className="pb-20 md:pb-28">
        <div className="container">
          <ul className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {features.map((feature) => (
              <li
                key={feature.title}
                className={`rounded-2xl p-6 ${feature.tone}`}
              >
                <feature.icon className="size-5" strokeWidth={1.5} />
                <h2 className="mt-5 font-display text-lg font-semibold tracking-tight">
                  {feature.title}
                </h2>
                <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
                  {feature.description}
                </p>
              </li>
            ))}
          </ul>
        </div>
      </section>

      <GlossaryPreview />
      <FinalCta />
      <SiteFooter />
    </div>
  );
}
