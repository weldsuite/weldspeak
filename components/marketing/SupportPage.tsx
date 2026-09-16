"use client";

import { Minus, Plus } from "lucide-react";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { FinalCta } from "@/components/marketing/FinalCta";
import { PageIntro } from "@/components/marketing/PageIntro";
import { SiteFooter } from "@/components/marketing/SiteFooter";
import { SiteHeader } from "@/components/marketing/SiteHeader";
import { faqs } from "@/lib/marketing-content";

export function SupportPage() {
  return (
    <div className="min-h-svh bg-background">
      <SiteHeader />
      <PageIntro
        label="Support"
        title="Good to know."
        description="Answers about platforms, accounts, the glossary, and how WeldSpeak fits with WeldSuite."
      />
      <section className="pb-20 md:pb-28">
        <div className="container max-w-3xl">
          <Accordion
            type="single"
            collapsible
            className="border-t border-border"
          >
            {faqs.map((item) => (
              <AccordionItem
                key={item.id}
                value={item.id}
                className="border-border"
              >
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
      <FinalCta />
      <SiteFooter />
    </div>
  );
}
