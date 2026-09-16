# WeldSpeak marketing site

Public marketing site for [WeldSpeak](https://weldspeak.com): landing, download, pricing, privacy, and sign-in.

Built with **Next.js 16 + Turbopack** (App Router). Product dashboard / desktop / API live in [`weldsuite/weldspeak`](https://github.com/weldsuite/weldspeak).

## Stack

- Next.js 16 (Turbopack for `dev` and `build`)
- React 19 + TypeScript
- Tailwind CSS v4 + shadcn/ui
- Clerk (`@clerk/nextjs`)
- Deploy target: **Vercel**

## Local development

```bash
pnpm install
cp .env.example .env.local
pnpm dev
```

`pnpm dev` runs `next dev --turbopack`. `pnpm build` runs `next build --turbopack`.

## Deploy (Vercel)

1. Import this repo in the Vercel dashboard (or `vercel --prod`).
2. Set env vars from `.env.example`.
3. Attach `weldspeak.com` and `www.weldspeak.com` (prefer www → apex redirect).

### API / dashboard co-hosting

Today `weldspeak.com` is a Cloudflare Worker that serves the SPA **and** `/api` + `/auth`. Pointing the apex at Vercel without moving the API will break device auth and the dashboard. Prefer attaching **www** first, then cut over apex after the API has its own host.

## Routes

| Path | Page |
| --- | --- |
| `/` | Landing |
| `/download` | Desktop downloads |
| `/pricing` | Pricing |
| `/privacy` | Privacy |
| `/sign-in` | Clerk sign-in |
