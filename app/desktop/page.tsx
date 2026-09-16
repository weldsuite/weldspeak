import type { Metadata } from "next";
import { DesktopPage } from "@/components/marketing/DesktopPage";

export const metadata: Metadata = {
  title: "Download WeldSpeak — Windows and macOS",
  description:
    "Download the WeldSpeak desktop app for Windows and macOS. Direct installers — hold a key, speak, let go.",
};

export default function DesktopRoute() {
  return <DesktopPage />;
}
