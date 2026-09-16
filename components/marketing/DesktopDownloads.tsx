import { Apple, Check, Download, Monitor } from "lucide-react";
import { DESKTOP_DOWNLOADS } from "@/components/marketing/links";

type Props = {
  version?: string | null;
  macUrl?: string | null;
  windowsUrl?: string | null;
};

/**
 * Desktop download cards matching weldsuite.org /desktop patterns.
 * Links use stable /api/download/* aliases that 302 to installer assets.
 */
export function DesktopDownloads({
  version = null,
  macUrl = DESKTOP_DOWNLOADS.mac,
  windowsUrl = DESKTOP_DOWNLOADS.windows,
}: Props) {
  const macHref = macUrl || DESKTOP_DOWNLOADS.mac;
  const winHref = windowsUrl || DESKTOP_DOWNLOADS.windows;

  return (
    <div className="container">
      <div className="download-grid grid gap-4 md:grid-cols-2">
        <article
          className="download-card flex flex-col rounded-2xl border border-border bg-card p-8 transition-colors hover:border-foreground/30"
          data-os="windows"
        >
          <span className="download-badge mb-4 hidden w-fit items-center gap-1.5 rounded-full border border-foreground/15 bg-background px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
            <Check className="size-3.5" strokeWidth={2.25} />
            Recommended for your device
          </span>
          <Monitor className="size-6" strokeWidth={1.5} />
          <h2 className="mt-5 font-display text-2xl font-semibold tracking-tight">
            WeldSpeak for Windows
          </h2>
          <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
            NSIS installer for Windows x64. Current-user install, no admin
            required.
          </p>
          <a
            href={winHref}
            className="mt-8 inline-flex w-fit items-center gap-2 rounded-md bg-foreground px-4 py-2.5 text-[13px] font-semibold text-background transition-opacity hover:opacity-90"
            download
          >
            <Monitor className="size-4" strokeWidth={1.75} />
            Download for Windows
            <Download className="size-3.5" />
          </a>
        </article>

        <article
          className="download-card flex flex-col rounded-2xl border border-border bg-card p-8 transition-colors hover:border-foreground/30"
          data-os="mac"
        >
          <span className="download-badge mb-4 hidden w-fit items-center gap-1.5 rounded-full border border-foreground/15 bg-background px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
            <Check className="size-3.5" strokeWidth={2.25} />
            Recommended for your device
          </span>
          <Apple className="size-6" strokeWidth={1.5} />
          <h2 className="mt-5 font-display text-2xl font-semibold tracking-tight">
            WeldSpeak for macOS
          </h2>
          <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
            Disk image for Apple silicon. Menu bar app with a global hotkey.
          </p>
          <a
            href={macHref}
            className="mt-8 inline-flex w-fit items-center gap-2 rounded-md bg-foreground px-4 py-2.5 text-[13px] font-semibold text-background transition-opacity hover:opacity-90"
            download
          >
            <Apple className="size-4" strokeWidth={1.75} />
            Download for macOS
            <Download className="size-3.5" />
          </a>
        </article>
      </div>
      {version ? (
        <p className="mt-6 text-center text-[13px] text-muted-foreground">
          Current version {version}
        </p>
      ) : null}
      <script
        dangerouslySetInnerHTML={{
          __html: `(function(){try{var u=navigator.userAgent||"";var p=/Mac|iPhone|iPad/.test(u)?"mac":/Win/.test(u)?"windows":"";if(!p)return;var g=document.querySelector(".download-grid");if(g)g.setAttribute("data-platform",p);document.querySelectorAll(".download-card").forEach(function(c){if(c.getAttribute("data-os")===p){c.classList.add("border-foreground");var b=c.querySelector(".download-badge");if(b)b.classList.remove("hidden"),b.classList.add("inline-flex");}});}catch(e){}})();`,
        }}
      />
    </div>
  );
}
