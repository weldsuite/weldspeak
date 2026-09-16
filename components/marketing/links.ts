export const DOWNLOAD_PAGE = "/download";
export const PRICING_PAGE = "/pricing";
/** Clerk plan price shown in marketing copy. */
export const PRICE_PER_PERSON_MONTH = 10;
/** Free-tier hard cap (calendar month, UTC). */
export const FREE_MONTHLY_WORD_CAP = 2000;
export const RELEASES_URL =
  "https://github.com/weldsuite/weldspeak/releases/latest";
export const GITHUB_URL = "https://github.com/weldsuite/weldspeak";
export const WELDSUITE_URL = "https://www.weldsuite.org/";
export const WELDSUITE_PRICING_URL = "https://www.weldsuite.org/pricing";
export const LOGO_SRC = "/icon.svg";

/**
 * Product dashboard (Clerk app) origin. Marketing is this repo; the dashboard
 * still lives in weldsuite/weldspeak until it gets its own host.
 * Override with NEXT_PUBLIC_DASHBOARD_ORIGIN when splitting domains.
 */
export const DASHBOARD_ORIGIN = (
  process.env.NEXT_PUBLIC_DASHBOARD_ORIGIN as string | undefined
)?.replace(/\/$/, "") || "https://weldspeak.com";

export const DASHBOARD_HOME = `${DASHBOARD_ORIGIN}/dictionary`;

/** Sign-in, then land on checkout. */
export const SIGN_IN_FOR_PRICING = `/sign-in?redirect_url=${encodeURIComponent(PRICING_PAGE)}`;
