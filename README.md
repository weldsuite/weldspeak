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

**Cleanup with a deadline.** The raw transcript is passed through a fast model
that removes fillers and false starts and fixes punctuation. It runs against a
hard 700 ms budget, and if it misses, the raw transcript ships instead. A
slightly scruffy result that arrives instantly beats a polished one that
arrives late.

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
apps/desktop              Tauri client: tray, hotkey, microphone, injection
```

## Setup

Needs Node 22+, pnpm 10+, and Rust stable. Building the desktop app also needs
the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your
platform.

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

### 3. Run it

```bash
pnpm --filter @weldspeak/web dev     # dashboard on :5173
pnpm --filter @weldspeak/api dev     # Worker on :8787
pnpm --filter @weldspeak/desktop-ui tauri dev
```

In the desktop app, set the API base to `http://localhost:8787` and sign in.

## Production domains

| Host | Role |
| --- | --- |
| `weldspeak.com` / `www` | Marketing site (Vercel, `weldsuite/weldspeak-marketing`) |
| `api.weldspeak.com` | API + dashboard Worker (`weldspeak-api`) |

Desktop default API base is `https://api.weldspeak.com`. Production Worker
`APP_URL` should be `https://api.weldspeak.com` so device-link pages stay on
the same host as `/api` and `/auth`.

### Cloudflare cutover (free apex for Vercel)

1. In Workers → `weldspeak-api` → Custom Domains / Triggers, **add**
   `api.weldspeak.com` (Cloudflare will create the DNS record).
2. Set production secret/var `APP_URL=https://api.weldspeak.com`.
3. Confirm `https://api.weldspeak.com/health` and `/api/me` work.
4. **Remove** the Worker custom domain on `weldspeak.com` (and any route that
   binds the apex to the Worker).
5. In Cloudflare DNS for the apex, set the records Vercel shows for
   `weldspeak.com` (typically A `216.150.1.1` / `216.150.16.1` or
   `76.76.21.21`) and CNAME `www` → `cname.vercel-dns.com` (or the
   project-specific target). Prefer DNS-only while verifying.
6. Vercel project domains: apex serves marketing; `www` → `weldspeak.com` (308).

Until step 4–5, leave apex on the Worker so the live API is not interrupted.

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
- **The Workers AI streaming call is written against documentation, not a live
  endpoint.** `@cf/deepgram/nova-3` over WebSocket is recent; the option names
  in `session-do.ts` are the single most likely thing here to have drifted. Run
  the `test:stream` checkpoint first.
- **Permissions flows are untested.** Microphone and Accessibility prompts
  behave differently for an app that has never been granted them, so test on a
  fresh macOS user account, not one where you have already clicked allow.

## Shipping

Distribution needs an Apple Developer account (notarization) and a Windows
code-signing certificate. Both have procurement lead time — start them early,
they are the usual reason a release slips.

macOS Accessibility permission resets when the app's signature changes, so keep
the signing identity stable across releases or every update silently breaks
injection for existing users.
