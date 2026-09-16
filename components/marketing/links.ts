export const DESKTOP_PAGE = "/desktop";
/** Legacy path — redirects to /desktop. */
export const DOWNLOAD_PAGE = "/download";
export const PRICING_PAGE = "/pricing";
export const FEATURES_PAGE = "/features";
export const HOW_IT_WORKS_PAGE = "/how-it-works";
export const SUPPORT_PAGE = "/support";

/** Clerk plan price shown in marketing copy. */
export const PRICE_PER_PERSON_MONTH = 10;
/** Free-tier hard cap (calendar month, UTC). */
export const FREE_MONTHLY_WORD_CAP = 2000;

export const GITHUB_URL = "https://github.com/weldsuite/weldspeak";
export const WELDSUITE_URL = "https://www.weldsuite.org/";
export const WELDSUITE_PRICING_URL = "https://www.weldsuite.org/pricing";
export const LOGO_SRC = "/icon.svg";

/**
 * Stable on-site download endpoints. Each 302s to the current installer asset
 * so the click starts a file download (never GitHub /releases/latest).
 */
export const DESKTOP_DOWNLOADS = {
  mac: "/api/download/mac",
  windows: "/api/download/windows",
} as const;

/**
 * Product dashboard origin. Override with NEXT_PUBLIC_DASHBOARD_ORIGIN.
 */
export const DASHBOARD_ORIGIN = (
  process.env.NEXT_PUBLIC_DASHBOARD_ORIGIN as string | undefined
)?.replace(/\/$/, "") || "https://api.weldspeak.com";

export const DASHBOARD_HOME = `${DASHBOARD_ORIGIN}/dictionary`;

/** Sign-in, then land on checkout. */
export const SIGN_IN_FOR_PRICING = `/sign-in?redirect_url=${encodeURIComponent(PRICING_PAGE)}`;
