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

/** Sign-in, then land on checkout. */
export const SIGN_IN_FOR_PRICING = `/sign-in?redirect_url=${encodeURIComponent(PRICING_PAGE)}`;
