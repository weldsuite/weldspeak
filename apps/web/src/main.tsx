import { ClerkProvider } from "@clerk/clerk-react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { ThemeProvider } from "./components/theme-provider.js";
import { App } from "./App.js";
import "./index.css";
import "./styles.css";

const publishableKey = import.meta.env.VITE_CLERK_PUBLISHABLE_KEY as string | undefined;

if (!publishableKey) {
  // Failing loudly beats a blank page: without this the app renders and every
  // Clerk component silently does nothing.
  throw new Error(
    "VITE_CLERK_PUBLISHABLE_KEY is not set. Copy apps/web/.env.example to .env.local.",
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* Same Clerk app as WeldSuite. The live publishable key encodes
        clerk.weldsuite.org as the Frontend API, so accounts and sessions are
        the WeldSuite ones. Serve the dashboard on *.weldsuite.org so the
        session cookie is shared with app.weldsuite.org. */}
    <ThemeProvider defaultTheme="light" storageKey="weldspeak-theme">
      <ClerkProvider
        publishableKey={publishableKey}
        afterSignOutUrl="/"
        signInUrl="/sign-in"
      >
        <BrowserRouter>
          <App />
        </BrowserRouter>
      </ClerkProvider>
    </ThemeProvider>
  </StrictMode>,
);
