import type { Metadata } from "next";
import { ClerkProvider } from "@clerk/nextjs";
import { ThemeProvider } from "@/components/theme-provider";
import "./globals.css";

export const metadata: Metadata = {
  title: "WeldSpeak — Hold a key, speak, let go",
  description:
    "Push-to-talk dictation for shops. Hold a key, speak, and cleaned-up text appears wherever you were typing. Streaming speech-to-text, team glossary, Windows and macOS.",
  openGraph: {
    title: "WeldSpeak — Hold a key, speak, let go",
    description:
      "Dictation that types into Slack, email, and your IDE. Built for alloy names, part numbers, and the words generic speech models mangle.",
    type: "website",
  },
  icons: {
    icon: [{ url: "/icon.svg", type: "image/svg+xml" }],
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  const publishableKey = process.env.NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY;

  if (!publishableKey) {
    throw new Error(
      "NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY is not set. Copy .env.example to .env.local.",
    );
  }

  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link
          rel="preconnect"
          href="https://fonts.gstatic.com"
          crossOrigin="anonymous"
        />
        <link
          href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&family=Manrope:wght@500;600;700&display=swap"
          rel="stylesheet"
        />
        <script
          dangerouslySetInnerHTML={{
            __html: `(function(){try{var s=localStorage.getItem("weldspeak-theme");var t;if(s==="dark"||s==="light"){t=s}else if(s==="system"){t=window.matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light"}else{t="light"}document.documentElement.classList.add(t)}catch(e){}})();`,
          }}
        />
      </head>
      <body>
        <ThemeProvider defaultTheme="light" storageKey="weldspeak-theme">
          <ClerkProvider
            publishableKey={publishableKey}
            afterSignOutUrl="/"
            signInUrl="/sign-in"
          >
            {children}
          </ClerkProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
