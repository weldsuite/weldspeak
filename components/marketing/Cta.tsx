"use client";

import type { ReactNode } from "react";
import Link from "next/link";
import { ArrowRight, ArrowUpRight } from "lucide-react";
import { cn } from "cn";

const base =
  "inline-flex items-center justify-center gap-1.5 rounded-[7px] px-3.5 py-[10px] text-[11px] font-semibold leading-none tracking-[0.01em] transition-opacity hover:opacity-90";

type CtaProps = {
  href: string;
  children: ReactNode;
  className?: string;
  external?: boolean;
};

function CtaLink({ href, external, className, children }: CtaProps) {
  if (external || href.startsWith("http")) {
    return (
      <a href={href} className={className} target="_blank" rel="noreferrer">
        {children}
      </a>
    );
  }
  // Hash anchors and download API aliases must be plain anchors (not client nav).
  if (
    href.startsWith("/#") ||
    href.startsWith("#") ||
    href.startsWith("/api/download/")
  ) {
    return (
      <a href={href} className={className}>
        {children}
      </a>
    );
  }
  return (
    <Link href={href} className={className}>
      {children}
    </Link>
  );
}

export function PrimaryCta({ href, children, className, external }: CtaProps) {
  return (
    <CtaLink
      href={href}
      external={external}
      className={cn(base, "bg-foreground text-background", className)}
    >
      {children}
      <ArrowUpRight className="size-3.5" strokeWidth={2.25} />
    </CtaLink>
  );
}

export function OutlineCta({ href, children, className, external }: CtaProps) {
  return (
    <CtaLink
      href={href}
      external={external}
      className={cn(
        base,
        "border border-border bg-transparent text-foreground",
        className,
      )}
    >
      {children}
      <ArrowRight className="size-3.5" strokeWidth={2.25} />
    </CtaLink>
  );
}
