import type { Metadata } from "next";
import { Suspense } from "react";
import { SignInPage } from "@/components/marketing/SignInPage";

export const metadata: Metadata = {
  title: "Sign in — WeldSpeak",
};

export default function SignInRoute() {
  return (
    <Suspense
      fallback={
        <div className="flex min-h-svh items-center justify-center text-muted-foreground">
          Loading…
        </div>
      }
    >
      <SignInPage />
    </Suspense>
  );
}
