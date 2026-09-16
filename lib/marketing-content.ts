import {
  BookOpen,
  Keyboard,
  Mic,
  Shield,
  Sparkles,
  Users,
  type LucideIcon,
} from "lucide-react";
import {
  FREE_MONTHLY_WORD_CAP,
  PRICE_PER_PERSON_MONTH,
} from "@/components/marketing/links";

export type Feature = {
  icon: LucideIcon;
  title: string;
  description: string;
  tone: string;
};

export const features: Feature[] = [
  {
    icon: Mic,
    title: "Streaming, not record-then-upload",
    description:
      "Audio goes to the recognizer while you speak, so the text is ready the moment you release the key.",
    tone: "bg-[#F8E4D4] dark:bg-card",
  },
  {
    icon: Sparkles,
    title: "Cleanup with a deadline",
    description:
      "Fillers and false starts come out. If the polish misses 2.5 seconds, the raw transcript still ships. Late is worse than scruffy.",
    tone: "bg-[#E4F0E8] dark:bg-card",
  },
  {
    icon: BookOpen,
    title: "A glossary that does work",
    description:
      "Alloy designations, customer names, part numbers. Fed to the recognizer as keyterm boosts and to cleanup as spelling context.",
    tone: "bg-[#F3E4EA] dark:bg-card",
  },
  {
    icon: Users,
    title: "Organizations, not personal accounts",
    description:
      "Shared vocabulary lives on the crew. Admins edit the org list; members get it the next time they dictate.",
    tone: "bg-[#E8E6F4] dark:bg-card",
  },
  {
    icon: Keyboard,
    title: "Windows and macOS",
    description:
      "A tray app with a global hotkey. Sign in once in the browser, approve the device, keep working in the tools you already have.",
    tone: "bg-[#F6EFE0] dark:bg-card",
  },
  {
    icon: Shield,
    title: "Your team, isolated",
    description:
      "Transcripts and glossaries are scoped to the organization. One shop cannot read another shop's words.",
    tone: "bg-[#E4EEF4] dark:bg-card",
  },
];

export const faqs = [
  {
    id: "faq-what",
    question: "What is WeldSpeak?",
    answer:
      "A push-to-talk dictation app. Hold a key, speak, let go. Cleaned-up text is typed into whichever app had focus — Slack, email, an IDE, a browser form. No copy-paste, no app switch.",
  },
  {
    id: "faq-why",
    question: "Why not use the dictation built into the OS?",
    answer:
      "Generic recognizers hear shop vocabulary and invent English. WeldSpeak streams to a speech model, then a cleanup pass, and both of those see your glossary: Inconel 625, customer names, P-numbers. That is the difference between usable traveler text and a mess you have to edit.",
  },
  {
    id: "faq-offline",
    question: "Does it work offline?",
    answer:
      "No. Recognition and cleanup run on Cloudflare Workers AI. The desktop app captures the microphone and injects text; the Worker does the listening.",
  },
  {
    id: "faq-platforms",
    question: "Which platforms?",
    answer:
      "Windows and macOS. Installers are built on GitHub Actions. You do not need Rust on the machine that will run WeldSpeak.",
  },
  {
    id: "faq-sign-in",
    question: "Why do I sign in on the website?",
    answer:
      "Accounts are Clerk organizations. Session cookies live in the browser, so the desktop app cannot hold one. You sign in here, the app shows a short code, you approve the device, and the Worker mints its own tokens.",
  },
  {
    id: "faq-glossary",
    question: "Who can edit the shared glossary?",
    answer:
      "Organization admins. Personal terms stay private. Shared terms are the high-value list — the words a whole crew needs spelled the same way.",
  },
  {
    id: "faq-price",
    question: "What does it cost?",
    answer: `Free is ${FREE_MONTHLY_WORD_CAP.toLocaleString("en-US")} words per calendar month. WeldSpeak is $${PRICE_PER_PERSON_MONTH} per person per month for uncapped words — each teammate who dictates pays their own subscription. If you already have a WeldSuite package, WeldSpeak is included.`,
  },
  {
    id: "faq-suite",
    question: "I already have WeldSuite. Do I need another subscription?",
    answer:
      "No. WeldSpeak is included with the WeldSuite package. Sign in with your WeldSuite account, download the app, and approve the device.",
  },
];
