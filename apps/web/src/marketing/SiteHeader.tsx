import { useState } from "react";
import { useAuth } from "@clerk/clerk-react";
import { Link, useLocation } from "react-router-dom";
import { Menu } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { Brand } from "@/marketing/Brand";
import { OutlineCta, PrimaryCta } from "@/marketing/Cta";
import { DOWNLOAD_PAGE } from "@/marketing/links";

const nav = [
  { title: "Product", href: "/#features" },
  { title: "How it works", href: "/#how-it-works" },
  { title: "Pricing", href: "/pricing" },
  { title: "FAQ", href: "/#faq" },
];

export function SiteHeader() {
  const { isSignedIn } = useAuth();
  const { pathname } = useLocation();
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
          {nav.map((item) => (
            <a
              key={item.href}
              href={item.href}
              className="text-[13px] font-medium text-foreground/80 transition-colors hover:text-foreground"
            >
              {item.title}
            </a>
          ))}
        </nav>

        <div className="hidden items-center gap-3 lg:flex">
          {isSignedIn ? (
            <Link
              to="/dictionary"
              className="px-1 text-[13px] font-medium text-foreground/80 hover:text-foreground"
            >
              Dashboard
            </Link>
          ) : onSignIn ? null : (
            <Link
              to="/sign-in"
              className="px-1 text-[13px] font-medium text-foreground/80 hover:text-foreground"
            >
              Log in
            </Link>
          )}
          {!isSignedIn && !onSignIn ? (
            <OutlineCta href="/sign-in">Sign in</OutlineCta>
          ) : null}
          <PrimaryCta href={DOWNLOAD_PAGE}>Download</PrimaryCta>
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
                <a
                  key={item.href}
                  href={item.href}
                  className="rounded-md px-3 py-2.5 text-[15px] font-medium"
                  onClick={() => setOpen(false)}
                >
                  {item.title}
                </a>
              ))}
            </nav>
            <div className="mt-auto flex flex-col gap-2 border-t border-border p-5">
              {isSignedIn ? (
                <OutlineCta href="/dictionary">Dashboard</OutlineCta>
              ) : (
                <OutlineCta href="/sign-in">Log in</OutlineCta>
              )}
              <PrimaryCta href={DOWNLOAD_PAGE}>Download</PrimaryCta>
            </div>
          </SheetContent>
        </Sheet>
      </div>
    </header>
  );
}
