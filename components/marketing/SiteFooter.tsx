"use client";

import Link from "next/link";
import { ModeToggle } from "@/components/mode-toggle";
import { Brand } from "@/components/marketing/Brand";
import {
  DASHBOARD_HOME,
  DOWNLOAD_PAGE,
  GITHUB_URL,
  PRICING_PAGE,
  WELDSUITE_URL,
} from "@/components/marketing/links";

const columns = [
  {
    title: "Product",
    links: [
      { name: "How it works", href: "/#how-it-works" },
      { name: "Features", href: "/#features" },
      { name: "Glossary", href: "/#glossary" },
      { name: "Pricing", href: PRICING_PAGE },
    ],
  },
  {
    title: "Get started",
    links: [
      { name: "Download", href: DOWNLOAD_PAGE },
      { name: "Sign in", href: "/sign-in" },
      { name: "Subscribe", href: PRICING_PAGE },
      { name: "Dashboard", href: DASHBOARD_HOME, external: true },
      { name: "Source", href: GITHUB_URL, external: true },
    ],
  },
  {
    title: "Company",
    links: [
      { name: "Privacy", href: "/privacy" },
      { name: "FAQ", href: "/#faq" },
      { name: "WeldSuite", href: WELDSUITE_URL, external: true },
      { name: "GitHub", href: GITHUB_URL, external: true },
    ],
  },
];

function FooterLink({
  href,
  name,
  external,
}: {
  href: string;
  name: string;
  external?: boolean;
}) {
  const className =
    "text-[13px] text-muted-foreground transition-colors hover:text-foreground";
  if (external) {
    return (
      <a href={href} className={className} target="_blank" rel="noreferrer">
        {name}
      </a>
    );
  }
  if (href.startsWith("/#") || href.startsWith("#")) {
    return (
      <a href={href} className={className}>
        {name}
      </a>
    );
  }
  return (
    <Link href={href} className={className}>
      {name}
    </Link>
  );
}

export function SiteFooter() {
  return (
    <footer className="border-t border-border">
      <div className="container py-16 md:py-20">
        <div className="grid gap-12 md:grid-cols-[1.2fr_repeat(3,1fr)]">
          <div>
            <Brand />
            <p className="mt-4 max-w-xs text-[13px] leading-relaxed text-muted-foreground">
              Hold a key, speak, let go. Dictation for shops that have a
              vocabulary.
            </p>
          </div>
          {columns.map((column) => (
            <div key={column.title}>
              <p className="mb-4 text-[11px] font-semibold uppercase tracking-[1.2px] text-foreground">
                {column.title}
              </p>
              <ul className="space-y-2.5">
                {column.links.map((link) => (
                  <li key={link.name}>
                    <FooterLink {...link} />
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
        <div className="mt-16 flex items-center justify-between border-t border-border pt-6">
          <p className="text-[12px] text-muted-foreground">
            © 2026 WeldSpeak. A WeldSuite product.
          </p>
          <ModeToggle />
        </div>
      </div>
    </footer>
  );
}
