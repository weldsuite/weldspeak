import type { Metadata } from "next";
import { DownloadPage } from "@/components/marketing/DownloadPage";

export const metadata: Metadata = {
  title: "Download WeldSpeak — Windows and macOS",
};

export default function DownloadRoute() {
  return <DownloadPage />;
}
