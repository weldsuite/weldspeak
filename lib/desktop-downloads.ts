/**
 * Resolve current WeldSpeak desktop installer URLs from the `desktop` GitHub
 * release (assets are overwritten in place on each publish).
 *
 * Marketing never links to `/releases/latest` or the releases index — only to
 * direct asset URLs (or our `/api/download/*` aliases that 302 to those assets).
 */

export type DesktopPlatform = "mac" | "windows";

export type DesktopInstallers = {
  version: string | null;
  mac: string | null;
  windows: string | null;
};

const RELEASE_API =
  "https://api.github.com/repos/weldsuite/weldspeak/releases/tags/desktop";

type GhAsset = { name: string; browser_download_url: string };

/** Stable on-site download aliases (start a download; never open a releases page). */
export const DESKTOP_DOWNLOAD_ALIASES = {
  mac: "/api/download/mac",
  windows: "/api/download/windows",
} as const;

export async function resolveDesktopInstallers(): Promise<DesktopInstallers> {
  try {
    const res = await fetch(RELEASE_API, {
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "weldspeak-marketing",
      },
      next: { revalidate: 300 },
    });
    if (!res.ok) {
      return fallbackInstallers();
    }
    const data = (await res.json()) as {
      tag_name?: string;
      assets?: GhAsset[];
    };
    const assets = data.assets ?? [];
    const mac =
      assets.find((a) => a.name.endsWith(".dmg"))?.browser_download_url ?? null;
    const windows =
      assets.find(
        (a) =>
          a.name.endsWith(".exe") &&
          !a.name.endsWith(".sig") &&
          a.name.includes("setup"),
      )?.browser_download_url ?? null;

    const version =
      mac?.match(/WeldSpeak_([^_]+)_/)?.[1] ??
      windows?.match(/WeldSpeak_([^_]+)_/)?.[1] ??
      null;

    if (!mac && !windows) return fallbackInstallers();
    return { version, mac, windows };
  } catch {
    return fallbackInstallers();
  }
}

/** Known-good installers if the GitHub API is unreachable at build/request time. */
function fallbackInstallers(): DesktopInstallers {
  return {
    version: "0.1.21",
    mac: "https://github.com/weldsuite/weldspeak/releases/download/desktop/WeldSpeak_0.1.21_aarch64.dmg",
    windows:
      "https://github.com/weldsuite/weldspeak/releases/download/desktop/WeldSpeak_0.1.21_x64-setup.exe",
  };
}
