# WeldSpeak

Hold a key, speak, let go. Cleaned-up text appears wherever you were typing —
Slack, an email, an IDE, a browser form. No copy-paste, no app switch.

Windows and macOS, backed by Cloudflare Workers AI, with teams through Clerk
Organizations.

## Why it is built this way

**Streaming, not record-then-upload.** Audio goes to the recognizer while you
speak, so the text is ready the moment you release the key. Uploading a
recording afterwards would add a second of dead air to every dictation, and
that second is the difference between a tool people use and one they abandon.

**Cleanup with a deadline.** The raw transcript is passed through a model that
removes fillers and false starts and fixes punctuation. It runs against a
2.5 s budget, and if it misses, the raw transcript ships instead. A slightly
scruffy result that arrives instantly beats a polished one that arrives late.

**Clerk stays in the browser.** Clerk session tokens live about a minute and
refresh through cookies on your own domain, so a desktop app cannot hold one.
The Worker runs an OAuth device grant and mints its own tokens instead. This is
why there is a web dashboard: it is where sign-in actually happens.

**Organizations do real work.** The shared glossary is the point — alloy
designations, customer names, part numbers. Fed to the recognizer as keyterm
boosts and to the cleanup model as spelling context, it is the difference
between "Inconel 625" and "in colonel six twenty five".

## Layout

```
packages/protocol         Wire protocol (TypeScript)
packages/protocol-rs      The same protocol, for Rust
packages/dictation-core   Portable client logic: audio, session, injection policy
workers/api               Cloudflare Worker: auth, dictation relay, org data
apps/web                  Dashboard: sign-in, device approval, team, glossary
apps/desktop              Hub webview + native tray, listening pill, hotkey, injection
```

## Setup

Needs Node 22+ and pnpm 10+. The desktop **installer** is built by GitHub
Actions — you do not install Rust or Visual Studio to use WeldSpeak. Rust and
the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) are only
required if you compile the desktop app on your own machine.

```bash
pnpm install
```

### 1. Cloudflare

Requires the Workers **Paid** plan — Durable Objects and Workers AI streaming
both need it.

```bash
cd workers/api
npx wrangler d1 create weldspeak            # put the id in wrangler.toml
npx wrangler kv namespace create DEVICE_CODES   # put the id in wrangler.toml
pnpm db:migrate:local
```

### 2. Clerk

Create an application, then enable **Organizations** under Organizations →
Settings. Under Roles & Permissions confirm `org:admin` and `org:member` exist.

```bash
cd workers/api
npx wrangler secret put CLERK_SECRET_KEY        # sk_...
npx wrangler secret put CLERK_PUBLISHABLE_KEY   # pk_...
npx wrangler secret put CLERK_WEBHOOK_SECRET    # whsec_... (see below)
npx wrangler secret put TOKEN_SIGNING_KEY       # openssl rand -base64 32
```

Add a webhook in Clerk pointing at `https://<your-worker>/webhooks/clerk`,
subscribed to `user.deleted`, `session.revoked` and
`organizationMembership.deleted`. This is the fast path for revocation; the
membership re-check on token refresh is the backstop for when a webhook is
dropped.

For the dashboard:

```bash
cp apps/web/.env.example apps/web/.env.local   # add your pk_...
```

### Billing

Per-person Clerk Billing (B2C): **$10 / person / month**, free tier **2,000
words / calendar month (UTC)**. Paid and WeldSuite-included users are uncapped.

Dashboard setup (plan slug `weldspeak`, features `unlimited_words` and
`weldsuite`) is documented in [`docs/clerk-billing.md`](docs/clerk-billing.md).
Checkout lives at `/pricing`.

### 3. Run it

The desktop Hub is a Tauri **webview** (Home, Dictionary, Snippets, Settings).
Dictation, hotkeys, injection, and the compact listening pill stay native.
Open WeldSpeak from the tray; hold the hotkey, speak, release, and the text
appears wherever you were typing.

```bash
pnpm --filter @weldspeak/web dev     # dashboard on :5173
pnpm --filter @weldspeak/api dev     # Worker on :8787
# Hub UI only (browser preview, no tray/dictation):
pnpm --filter @weldspeak/desktop-ui dev
# Full desktop app (needs Tauri prerequisites):
pnpm --filter @weldspeak/desktop-ui tauri dev
```

**Installed builds ship from `ci/desktop` only.** Merging Hub CSS to `main`
does not update Gert’s DMG/EXE. After this lands on `ci/desktop`, run
**Actions → Desktop installers** (or push `ci/desktop`) so a newer `0.1.x`
lands on the rolling `desktop` release / `latest.json`, then relaunch.

The desktop app talks to `https://weldspeak.com` by default. For a local
Worker, point `apiBase` at `http://localhost:8787` in
`%APPDATA%\io.weldspeak.desktop\settings.json`.

## Desktop installers

GitHub Actions builds the Windows `.exe` and macOS `.dmg`. You never need
Rust on the machine that will run WeldSpeak.

- **Actions → Desktop installers → Run workflow** — grab the artifact
- **git tag `v0.1.0` and push** — same files, plus a GitHub Release

Unsigned Windows builds will trip SmartScreen until a code-signing certificate
is in the workflow. That is expected for a first personal install: More info →
Run anyway.

### In-app updates

Installed apps read
`https://github.com/weldsuite/weldspeak/releases/download/desktop/latest.json`
(Tauri updater). On launch the desktop process checks quietly and installs when
a newer signed build is present. The Hub also probes the same feed and shows an
**Update** control when a newer version is available; clicking it runs the same
download → install → restart path.

Repo secrets required for updater artifacts (set in GitHub → Settings →
Secrets):

| Secret | Purpose |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Minisign private key; enables `--config bundle.createUpdaterArtifacts` in Desktop installers |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for that key (omit if the key is unencrypted) |

The matching **public** key is committed in
`apps/desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`. Without the
private key, CI still builds installers but skips `.sig` / updater tarballs and
cannot refresh `latest.json`.

## Production

Deployed to the `WeldSuite` Cloudflare account as the Worker `weldspeak-api`,
serving the API, the dictation WebSocket and the dashboard from one hostname:

```
https://weldspeak.com                              custom domain
https://weldspeak-api.fragrant-cake-015a.workers.dev   fallback
```

The `workers.dev` hostname is kept enabled alongside the custom domain, so
there is still a way in if the domain is mid-migration or a certificate is
provisioning.

### Choosing the cleanup model

`CLEANUP_MODEL` is `@cf/meta/llama-4-scout-17b-16e-instruct`. It clears the
2.5 s cleanup deadline with room to spare, follows rewrite instructions well,
and thinking is turned off on the request so a reasoning trace cannot leak into
the inserted text.

The glossary still matters for the recognizer: with `Inconel 625` in
`dictionary_terms`, keyterm boost yields the term before cleanup even runs.

## Testing

```bash
pnpm --filter @weldspeak/api test    # 71 tests
cargo test --workspace               # 75 tests
```

The Worker suite covers token forgery and expiry, device-grant replay, refresh
reuse detection, membership revocation, cleanup failure modes, and — most
importantly for a team product — cross-organization isolation: that a member of
one org cannot read another's glossary or transcripts, and that a plain member
is refused a shared-glossary write.

The Rust suite covers the parts hardest to check by hand, notably that a 15 kHz
tone is suppressed rather than folded into the speech band by the resampler,
and the session state machine's timing races.

### Verifying the Cloudflare path

Before trusting anything end to end, prove the server side alone:

```bash
# 16 kHz mono 16-bit WAV: ffmpeg -i in.wav -ar 16000 -ac 1 -c:a pcm_s16le out.wav
WELDSPEAK_TOKEN=<access token> pnpm --filter @weldspeak/api test:stream out.wav
```

It streams the file in real time and prints partials, the raw transcript, the
cleaned result, and the latency of each stage.

## What has not been verified

This was built and tested in a Linux container. Everything above that is
claimed to pass, passes. The following has **never been run on real hardware**
and should be treated as unverified:

- **The desktop app has not been launched.** It compiles clean and clippy is
  clean, but compiling is not running. Expect to debug the first launch.
- **Text injection is untested against real applications.** The plan's
  verification matrix — TextEdit/Notepad, Chrome, VS Code, Slack, one Electron
  app, checking accents and emoji survive and the clipboard is restored — still
  needs doing on both platforms.
- **Hotkey capture is unproven.** Tauri's global-shortcut plugin reports press
  and release, which is what push-to-talk needs, but its coverage of held
  modifier keys varies by platform. If Right Option does not report a release
  on macOS, that path needs a native `NSEvent` monitor instead.
- ~~**The Workers AI streaming call is written against documentation, not a live
  endpoint.**~~ Verified against the live endpoint. It had drifted, in exactly
  the place predicted: Workers AI validates the options payload as all-strings
  and rejects a number or boolean with a 400 (`expected a string`), so
  `sample_rate: 16000` and `channels: 1` failed the handshake. Every scalar is
  now sent as its string form; `keyterm` stays an array. See `#connectUpstream`
  in `session-do.ts`.
- **Permissions flows are untested.** Microphone and Accessibility prompts
  behave differently for an app that has never been granted them, so test on a
  fresh macOS user account, not one where you have already clicked allow.

## Shipping

Installers are produced by `.github/workflows/desktop.yml` on
`windows-latest` and `macos-latest`. Those runners already have Rust and the
C++ toolchain.

Distribution to other people needs an Apple Developer account (notarization)
and a Windows code-signing certificate. Both have procurement lead time —
start them early, they are the usual reason a release slips.

macOS Accessibility permission resets when the app's signature changes, so keep
the signing identity stable across releases or every update silently breaks
injection for existing users.
