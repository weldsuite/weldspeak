"use client";

import Link from "next/link";
import { cn } from "cn";
import { LOGO_SRC } from "@/components/marketing/links";

export function Brand({ className }: { className?: string }) {
  return (
    <Link
      href="/"
      aria-label="WeldSpeak home"
      className={cn("flex items-center gap-2.5", className)}
    >
      <img src={LOGO_SRC} alt="" className="size-7" />
      <span className="font-display text-[17px] font-semibold tracking-tight">
        WeldSpeak
      </span>
    </Link>
  );
}
