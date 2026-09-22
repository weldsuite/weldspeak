/**
 * Publish the rolling `desktop` GitHub Release and the updater manifest.
 *
 * The updater plugin reads `latest.json` from that release. Publish whichever
 * signed platforms this run produced — Windows users should not wait on macOS
 * updater artifacts, and vice versa.
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

const platforms = {};
const upload = [];

if (windowsSetup && windowsSig) {
  platforms["windows-x86_64"] = {
    url: `${BASE}/${windowsSetup.name}`,
    signature: (await readFile(windowsSig.path, "utf8")).trim(),
  };
  upload.push(windowsSetup.path, windowsSig.path);
}

if (macArchive && macSig) {
  platforms["darwin-aarch64"] = {
    url: `${BASE}/${macArchive.name}`,
    signature: (await readFile(macSig.path, "utf8")).trim(),
  };
  upload.push(macArchive.path, macSig.path);
}

if (!version || Object.keys(platforms).length === 0) {
  console.error(
    "No signed updater artifacts to publish. Windows needs x64-setup.exe + .sig; macOS needs .app.tar.gz + .sig.",
  );
  process.exit(1);
}

const latest = {
  version,
  notes: "Latest WeldSpeak desktop build.",
  pub_date: new Date().toISOString(),
  platforms,
};

const latestPath = path.join(ROOT, "latest.json");
await writeFile(latestPath, `${JSON.stringify(latest, null, 2)}\n`);
upload.push(latestPath);

const already = new Set(upload);
for (const file of files) {
  if (/\.(exe|dmg)$/i.test(file.name) && !already.has(file.path)) {
    upload.push(file.path);
  }
}

console.log(
  `Publishing WeldSpeak ${version} for ${Object.keys(platforms).join(", ")}`,
);

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
    // Pin the rolling tag to the installer commit (ci/desktop SHA), not default main.
    "--target",
    process.env.GITHUB_SHA || "ci/desktop",
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
