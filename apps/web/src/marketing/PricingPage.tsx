import { PricingTable, SignedIn, SignedOut } from "@clerk/clerk-react";
import { OutlineCta, PrimaryCta } from "@/marketing/Cta";
import {
  FREE_MONTHLY_WORD_CAP,
  PRICE_PER_PERSON_MONTH,
  PRICING_PAGE,
} from "@/marketing/links";
import { SiteFooter } from "@/marketing/SiteFooter";
import { SiteHeader } from "@/marketing/SiteHeader";

const signInForPricing = `/sign-in?redirect_url=${encodeURIComponent(PRICING_PAGE)}`;

export function PricingPage() {
  return (
    <div className="flex min-h-svh flex-col bg-background">
      <SiteHeader />
      <main className="flex-1 py-16 md:py-24">
        <div className="container max-w-3xl">
          <h1 className="font-display text-4xl font-semibold tracking-[-0.05em] text-pretty md:text-5xl">
            ${PRICE_PER_PERSON_MONTH} per person / month
          </h1>
          <p className="mt-4 max-w-xl text-[14px] leading-relaxed text-muted-foreground">
            Free includes {FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words
            per calendar month. Paid and WeldSuite-included accounts are
            uncapped. Each teammate who dictates pays their own subscription —
            not a flat org fee.
          </p>

          <SignedOut>
            <div className="mt-10 flex flex-wrap gap-3">
              <PrimaryCta href={signInForPricing}>Sign in to subscribe</PrimaryCta>
              <OutlineCta href="/#pricing">Compare plans</OutlineCta>
            </div>
            <p className="mt-4 text-[13px] text-muted-foreground">
              Checkout opens after you sign in with your WeldSuite account.
            </p>
          </SignedOut>

          <SignedIn>
            <div className="mt-10">
              <PricingTable for="user" newSubscriptionRedirectUrl="/usage" />
            </div>
            <p className="mt-6 text-[13px] text-muted-foreground">
              Manage or cancel anytime from your account menu (Billing). Already
              on WeldSuite? You should not need a second charge — contact us if
              unlimited words is missing.
            </p>
          </SignedIn>
        </div>
      </main>
      <SiteFooter />
    </div>
  );
}
