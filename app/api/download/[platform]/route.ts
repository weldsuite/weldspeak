import { NextResponse } from "next/server";
import {
  resolveDesktopInstallers,
  type DesktopPlatform,
} from "@/lib/desktop-downloads";

export const runtime = "nodejs";
export const revalidate = 300;

const PLATFORMS = new Set<DesktopPlatform>(["mac", "windows"]);

/**
 * Stable download aliases. 302 to the current installer asset so the browser
 * starts a file download — never an HTML releases index.
 */
export async function GET(
  _request: Request,
  context: { params: Promise<{ platform: string }> },
) {
  const { platform: raw } = await context.params;
  if (!PLATFORMS.has(raw as DesktopPlatform)) {
    return NextResponse.json({ error: "Unknown platform" }, { status: 404 });
  }
  const platform = raw as DesktopPlatform;
  const installers = await resolveDesktopInstallers();
  const url = installers[platform];
  if (!url) {
    return NextResponse.json(
      { error: "Installer not available" },
      { status: 404 },
    );
  }
  return NextResponse.redirect(url, 302);
}
