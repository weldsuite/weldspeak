import type { Metadata } from "next";
import { FeaturesPage } from "@/components/marketing/FeaturesPage";

export const metadata: Metadata = {
  title: "Features — WeldSpeak",
  description:
    "Streaming speech-to-text, cleanup on a deadline, and a shop glossary that knows alloy names from English.",
};

export default function FeaturesRoute() {
  return <FeaturesPage />;
}
