import { OutlineCta, PrimaryCta } from "@/components/marketing/Cta";
import { DESKTOP_PAGE } from "@/components/marketing/links";

export function FinalCta({
  title = "Talk. It types.",
  body = "Download the app, sign in, approve the device, add the terms your shop actually says. That is the whole setup.",
}: {
  title?: string;
  body?: string;
}) {
  return (
    <section className="py-20 md:py-28">
      <div className="container text-center">
        <h2 className="font-display text-5xl leading-[0.98] font-semibold tracking-[-0.06em] text-pretty lg:text-[5.5rem]">
          {title}
        </h2>
        <p className="mx-auto mt-6 max-w-md text-[14px] leading-relaxed text-muted-foreground">
          {body}
        </p>
        <div className="mt-8 flex flex-wrap items-center justify-center gap-2.5">
          <PrimaryCta href={DESKTOP_PAGE}>Download</PrimaryCta>
          <OutlineCta href="/sign-in">Sign in</OutlineCta>
        </div>
      </div>
    </section>
  );
}
