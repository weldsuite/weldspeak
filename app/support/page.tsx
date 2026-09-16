import type { Metadata } from "next";
import { SupportPage } from "@/components/marketing/SupportPage";

export const metadata: Metadata = {
  title: "Support — WeldSpeak",
  description:
    "FAQ about WeldSpeak platforms, accounts, glossary, pricing, and WeldSuite.",
};

export default function SupportRoute() {
  return <SupportPage />;
}
