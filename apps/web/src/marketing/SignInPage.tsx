import { SignIn } from "@clerk/clerk-react";
import { useSearchParams } from "react-router-dom";
import { SiteFooter } from "@/marketing/SiteFooter";
import { SiteHeader } from "@/marketing/SiteHeader";

export function SignInPage() {
  const [params] = useSearchParams();
  const redirect = params.get("redirect_url") || "/dictionary";

  return (
    <div className="flex min-h-svh flex-col bg-background">
      <SiteHeader />
      <div className="flex flex-1 flex-col items-center justify-center gap-8 px-6 py-16">
        <div className="text-center">
          <h1 className="font-display text-3xl font-semibold tracking-tight">
            Sign in
          </h1>
          <p className="mt-2 max-w-sm text-[14px] text-muted-foreground">
            Same account as the rest of WeldSuite. After this, approve the
            desktop app with a short code.
          </p>
        </div>
        <SignIn
          routing="path"
          path="/sign-in"
          forceRedirectUrl={redirect}
          signUpForceRedirectUrl={redirect}
        />
      </div>
      <SiteFooter />
    </div>
  );
}
