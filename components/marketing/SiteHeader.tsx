"use client";

import { useState } from "react";
import { useAuth } from "@clerk/nextjs";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Menu } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { Brand } from "@/components/marketing/Brand";
import { OutlineCta, PrimaryCta } from "@/components/marketing/Cta";
import {
  DASHBOARD_HOME,
  DESKTOP_PAGE,
  FEATURES_PAGE,
  HOW_IT_WORKS_PAGE,
  PRICING_PAGE,
  SUPPORT_PAGE,
} from "@/components/marketing/links";

const nav = [
  { title: "Features", href: FEATURES_PAGE },
  { title: "How it works", href: HOW_IT_WORKS_PAGE },
  { title: "Pricing", href: PRICING_PAGE },
  { title: "Desktop", href: DESKTOP_PAGE },
  { title: "Support", href: SUPPORT_PAGE },
];

export function SiteHeader() {
  const { isSignedIn } = useAuth();
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const onSignIn = pathname.startsWith("/sign-in");

  return (
    <header className="sticky top-0 z-50 border-b border-border bg-background/94 backdrop-blur-[16px]">
      <div className="container flex h-[72px] items-center justify-between lg:h-[85px]">
        <Brand />

        <nav
          aria-label="Main navigation"
          className="hidden items-center gap-7 lg:flex"
        >
          {nav.map((item) => {
            const active =
              pathname === item.href || pathname.startsWith(`${item.href}/`);
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`text-[13px] font-medium transition-colors hover:text-foreground ${
                  active ? "text-foreground" : "text-foreground/80"
                }`}
              >
                {item.title}
              </Link>
            );
          })}
        </nav>

        <div className="hidden items-center gap-3 lg:flex">
          {isSignedIn ? (
            <a
              href={DASHBOARD_HOME}
              className="px-1 text-[13px] font-medium text-foreground/80 hover:text-foreground"
            >
              Dashboard
            </a>
          ) : onSignIn ? null : (
            <Link
              href="/sign-in"
              className="px-1 text-[13px] font-medium text-foreground/80 hover:text-foreground"
            >
              Log in
            </Link>
          )}
          {!isSignedIn && !onSignIn ? (
            <OutlineCta href="/sign-in">Get started</OutlineCta>
          ) : null}
          <PrimaryCta href={DESKTOP_PAGE}>Download</PrimaryCta>
        </div>

        <Sheet open={open} onOpenChange={setOpen}>
          <SheetTrigger asChild>
            <Button
              variant="outline"
              size="icon"
              className="lg:hidden"
              aria-label="Open menu"
            >
              <Menu className="size-4" />
            </Button>
          </SheetTrigger>
          <SheetContent side="right" className="w-[min(100%,20rem)] p-0">
            <SheetHeader className="border-b border-border px-5 py-4">
              <SheetTitle className="sr-only">Menu</SheetTitle>
              <Brand />
            </SheetHeader>
            <nav className="flex flex-col gap-1 px-3 py-4">
              {nav.map((item) => (
                <Link
                  key={item.href}
                  href={item.href}
                  className="rounded-md px-3 py-2.5 text-[15px] font-medium"
                  onClick={() => setOpen(false)}
                >
                  {item.title}
                </Link>
              ))}
            </nav>
            <div className="mt-auto flex flex-col gap-2 border-t border-border p-5">
              {isSignedIn ? (
                <OutlineCta href={DASHBOARD_HOME}>Dashboard</OutlineCta>
              ) : (
                <OutlineCta href="/sign-in">Log in</OutlineCta>
              )}
              <PrimaryCta href={DESKTOP_PAGE}>Download</PrimaryCta>
            </div>
          </SheetContent>
        </Sheet>
      </div>
    </header>
  );
}
