import { FinalCta } from "@/components/marketing/FinalCta";
import { HowItWorks } from "@/components/marketing/HowItWorks";
import { PageIntro } from "@/components/marketing/PageIntro";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";
import { WhereItTypes } from "@/components/marketing/WhereItTypes";

export function HowItWorksPage() {
  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />
      <PageIntro
        label="Less typing. More welding."
        title="Hold. Speak. Let go."
        description="Dictation that keeps up with a shop floor, not a meeting transcript you clean up later."
      />
      <WhereItTypes />
      <HowItWorks />
      <FinalCta />
      <SiteFooter />
    </div>
  );
}
