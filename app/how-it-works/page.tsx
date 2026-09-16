import type { Metadata } from "next";
import { HowItWorksPage } from "@/components/marketing/HowItWorksPage";

export const metadata: Metadata = {
  title: "How it works — WeldSpeak",
  description:
    "Hold a key, speak, let go. Streaming recognition and text injected where the cursor already was.",
};

export default function HowItWorksRoute() {
  return <HowItWorksPage />;
}
