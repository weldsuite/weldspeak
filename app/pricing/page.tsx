import type { Metadata } from "next";
import { PricingPage } from "@/components/marketing/PricingPage";

export const metadata: Metadata = {
  title: "Pricing — WeldSpeak",
};

export default function PricingRoute() {
  return <PricingPage />;
}
