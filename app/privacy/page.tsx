import type { Metadata } from "next";
import { PrivacyPage } from "@/components/marketing/PrivacyPage";

export const metadata: Metadata = {
  title: "Privacy — WeldSpeak",
};

export default function PrivacyRoute() {
  return <PrivacyPage />;
}
