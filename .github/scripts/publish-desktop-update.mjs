/**
 * Publish the rolling `desktop` GitHub Release and the updater manifest.
 *
 * The updater plugin reads `latest.json` from that release. Both platform
 * jobs must succeed first so the file always lists a complete set of URLs.
 */

import { readdir, readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";

const OWNER_REPO = process.env.GITHUB_REPOSITORY;
const TOKEN = process.env.GITHUB_TOKEN ?? process.env.GH_TOKEN;
const ROOT = process.argv[2] ?? "artifacts";
const TAG = "desktop";
const BASE = `https://github.com/${OWNER_REPO}/releases/download/${TAG}`;

if (!OWNER_REPO || !TOKEN) {
  throw new Error("GITHUB_REPOSITORY and GITHUB_TOKEN are required");
}

const files = await collect(ROOT);
const windowsSetup = files.find((file) => /x64-setup\.exe$/i.test(file.name));
const windowsSig = files.find((file) => /x64-setup\.exe\.sig$/i.test(file.name));
const macArchive = files.find((file) => /\.app\.tar\.gz$/i.test(file.name));
const macSig = files.find((file) => /\.app\.tar\.gz\.sig$/i.test(file.name));
const versionMatch = (windowsSetup ?? macArchive)?.name.match(/_(\d+\.\d+\.\d+)_/);
const version = versionMatch?.[1];

if (!windowsSetup || !windowsSig || !macArchive || !macSig || !version) {
  console.log("Skipping updater release: signed artifacts for both platforms were not present.");
  process.exit(0);
}

const latest = {
  version,
  notes: "Latest WeldSpeak desktop build.",
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      url: `${BASE}/${windowsSetup.name}`,
      signature: (await readFile(windowsSig.path, "utf8")).trim(),
    },
    "darwin-aarch64": {
      url: `${BASE}/${macArchive.name}`,
      signature: (await readFile(macSig.path, "utf8")).trim(),
    },
  },
};

const latestPath = path.join(ROOT, "latest.json");
await writeFile(latestPath, `${JSON.stringify(latest, null, 2)}\n`);

const installers = files.filter((file) => /\.(exe|dmg)$/i.test(file.name));
const upload = [
  windowsSetup.path,
  windowsSig.path,
  macArchive.path,
  macSig.path,
  ...installers
    .filter((file) => file.path !== windowsSetup.path)
    .map((file) => file.path),
  latestPath,
];

try {
  execFileSync("gh", ["release", "delete", TAG, "--yes", "--cleanup-tag"], {
    stdio: "inherit",
    env: process.env,
  });
} catch {
  try {
    execFileSync("gh", ["release", "delete", TAG, "--yes"], {
      stdio: "inherit",
      env: process.env,
    });
  } catch {
    // First publish, or the tag was already gone.
  }
}

execFileSync(
  "gh",
  [
    "release",
    "create",
    TAG,
    "--prerelease",
    "--title",
    `WeldSpeak ${version}`,
    "--notes",
    `Rolling desktop build ${version}. Installed apps pick this up on the next launch.`,
    ...upload,
  ],
  { stdio: "inherit", env: process.env },
);

async function collect(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const found = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await collect(full)));
    } else {
      found.push({ name: entry.name, path: full });
    }
  }
  return found;
}
